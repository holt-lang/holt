use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use clap::builder::styling::{AnsiColor, Color as ClapColor, Style, Styles};
use console::{Color, style};
use indicatif::{ProgressBar, ProgressStyle};
use miette::Report;

use compiler::lexer::lex;

/// Standard-library sources embedded at build time by `holt/build.rs`
/// (`stdlib/**/*.hlt`), used by `holt setup` to populate `~/.hella/lib`.
include!(concat!(env!("OUT_DIR"), "/stdlib_embedded.rs"));

/// Holt brand green #00A693 as an ANSI truecolor style.
fn brand_style() -> Style {
    Style::new().fg_color(Some(ClapColor::Rgb(anstyle::RgbColor(0x00, 0xA6, 0x93)))).bold()
}

/// clap help/error styling in the Holt brand palette:
/// brand-green headers and literals, yellow errors, dim context.
fn cli_styles() -> Styles {
    let brand = brand_style();
    let dim = Style::new().fg_color(Some(ClapColor::Ansi(AnsiColor::BrightBlack)));
    let yellow = Style::new().fg_color(Some(ClapColor::Ansi(AnsiColor::Yellow)));
    Styles::styled()
        .header(brand)
        .usage(brand)
        .literal(brand)
        .placeholder(dim)
        .context(dim)
        .context_value(Style::new())
        .error(yellow.bold())
        .invalid(yellow.bold())
        .valid(brand)
}

/// Holt — main entry point (wraps `compiler` crate)
#[derive(Parser, Debug)]
#[command(
    name = "holt",
    version = "0.1.0",
    about = "Holt compiler",
    long_about = "Holt compiler — build, check and run .hlt programs.",
    styles = cli_styles(),
    arg_required_else_help = true,
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build a .hlt source file (lex → parse → check → codegen → link)
    #[command(visible_alias = "b")]
    Build(BuildArgs),
    /// Build and run a .hlt source file, removing the binary afterwards
    #[command(visible_alias = "r")]
    Run(RunArgs),
    /// Check a .hlt source file (lex → parse → check, no codegen)
    #[command(visible_alias = "c")]
    Check(CheckArgs),
    /// Run the Holt language server (LSP over stdio)
    #[command(visible_alias = "ls")]
    Lsp,
    /// Install the embedded standard library to `~/.hella/lib`
    Setup(SetupArgs),
}

#[derive(Parser, Debug)]
struct BuildArgs {
    /// Source file (.hlt) to compile
    file: PathBuf,

    /// Emit LLVM IR to stdout and exit (no link)
    #[arg(long, default_value_t = false)]
    emit_llvm: bool,

    /// When --emit-llvm, write IR to file instead of stdout
    #[arg(long)]
    emit_llvm_file: Option<PathBuf>,

    /// Keep object file (don't delete after linking)
    #[arg(long, default_value_t = false)]
    keep_obj: bool,

    /// Output executable path (default: input file with extension stripped)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Print AST for debugging
    #[arg(long, default_value_t = false)]
    print_ast: bool,

    /// Optimized release build (O3 IR passes + aggressive codegen)
    #[arg(long, default_value_t = false)]
    release: bool,

    /// Only print errors and program stdout (no status lines or progress bar)
    #[arg(long, default_value_t = false)]
    quiet: bool,

    /// Force coloured output even when stderr is not a terminal
    /// (also honours the `FORCE_COLOR` environment variable)
    #[arg(long, default_value_t = false)]
    color: bool,

    /// Show verbose progress (default: summary lines)
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

#[derive(Parser, Debug)]
struct RunArgs {
    /// Source file (.hlt) to compile and run
    file: PathBuf,

    /// Keep object file (don't delete after linking)
    #[arg(long, default_value_t = false)]
    keep_obj: bool,

    /// Print AST for debugging
    #[arg(long, default_value_t = false)]
    print_ast: bool,

    /// Optimized release build (O3 IR passes + aggressive codegen)
    #[arg(long, default_value_t = false)]
    release: bool,

    /// Only print errors and program stdout (no status lines or progress bar)
    #[arg(long, default_value_t = false)]
    quiet: bool,

    /// Force coloured output even when stderr is not a terminal
    /// (also honours the `FORCE_COLOR` environment variable)
    #[arg(long, default_value_t = false)]
    color: bool,

    /// Show verbose progress (default: summary lines)
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Arguments forwarded to the program (use `--` to separate them)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    program_args: Vec<String>,
}

#[derive(Parser, Debug)]
struct SetupArgs {
    /// Overwrite files that already exist in `~/.hella/lib`
    #[arg(long, default_value_t = false)]
    force: bool,
}

#[derive(Parser, Debug)]
struct CheckArgs {
    /// Source file (.hlt) to check
    file: PathBuf,

    /// Print AST for debugging
    #[arg(long, default_value_t = false)]
    print_ast: bool,

    /// Only print errors (no status lines or progress bar)
    #[arg(long, default_value_t = false)]
    quiet: bool,

    /// Force coloured output even when stderr is not a terminal
    /// (also honours the `FORCE_COLOR` environment variable)
    #[arg(long, default_value_t = false)]
    color: bool,

    /// Show verbose progress (default: summary lines)
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

/// Shared knobs for the compile pipeline (used by `build`, `run` and `check`).
struct CompileOptions<'a> {
    file: &'a Path,
    emit_llvm: bool,
    emit_llvm_file: Option<&'a Path>,
    keep_obj: bool,
    print_ast: bool,
    release: bool,
    quiet: bool,
    force_color: bool,
    verbose: bool,
    exe_path: Option<PathBuf>,
    /// Stop after sema (no codegen/link). Used by `check`.
    check_only: bool,
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build(args) => run_build(args),
        Commands::Run(args) => run_run(args),
        Commands::Check(args) => run_check(args),
        // The language server speaks LSP on stdio and terminates itself with
        // its own exit code after the client sends `exit`.
        Commands::Lsp => std::process::exit(hls::server::run()),
        Commands::Setup(args) => run_setup(args),
    }
}

/// Install the embedded standard library (`stdlib/**/*.hlt` baked in by
/// `holt/build.rs`) to `~/.hella/lib`, the directory the compiler searches
/// for `import`ed libraries. Existing files are kept unless `--force`.
fn run_setup(args: SetupArgs) -> miette::Result<()> {
    let Some(dest_root) = compiler::modules::hella_lib_dir() else {
        return Err(miette::miette!(
            "`holt setup` is only supported on UNIX-like systems for now"
        ));
    };
    let mut installed = 0usize;
    let mut skipped = 0usize;
    for (rel, contents) in STDLIB_FILES {
        let dest = dest_root.join(rel);
        if dest.is_file() && !args.force {
            skipped += 1;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                miette::miette!("failed to create {}: {e}", parent.display())
            })?;
        }
        fs::write(&dest, contents)
            .map_err(|e| miette::miette!("failed to write {}: {e}", dest.display()))?;
        installed += 1;
    }
    eprintln!(
        "{:>11} stdlib → {} ({installed} installed, {skipped} kept)",
        brand("Setup"),
        gpath(&dest_root),
    );
    Ok(())
}

/// Toolchain terminal output: a single progress bar covering the whole
/// compile, plus one static status line per completed phase on stderr with a
/// right-aligned brand-green prefix (`{:>11}`, `Compiled in 0.12s`). `--quiet`
/// silences everything except errors and program stdout.
fn init_colors(force: bool) {
    if force
        || std::env::var("FORCE_COLOR")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    {
        console::set_colors_enabled(true);
    } else if !std::io::stderr().is_terminal() {
        console::set_colors_enabled(false);
    }
}

/// Brand color: Holt green #00A693 (RGB 0, 166, 147).
fn brand(prefix: &str) -> String {
    style(prefix)
        .fg(Color::TrueColor(0x00, 0xA6, 0x93))
        .bold()
        .to_string()
}

/// Brand-green path for the summary line.
fn gpath(p: &Path) -> String {
    style(p.display().to_string())
        .fg(Color::TrueColor(0x00, 0xA6, 0x93))
        .to_string()
}

/// Yellow duration, matching compiler-error yellow accents.
fn gduration(d: Duration) -> String {
    style(seconds(d))
        .fg(Color::Yellow)
        .to_string()
}

/// Formats durations as `0.12s`.
fn seconds(d: Duration) -> String {
    format!("{:.2}s", d.as_millis() as f32 / 1000.)
}

/// Single status line on stderr: brand-green right-aligned prefix + message.
/// Suppressed under `--quiet`. Suspends the progress bar so the bar redraws
/// cleanly instead of being corrupted by raw `eprintln!` output.
fn status(pb: &ProgressBar, quiet: bool, prefix: &str, msg: &str) {
    if !quiet {
        pb.suspend(|| eprintln!("{:>11} {}", brand(prefix), msg));
    }
}

/// Single progress bar for the whole compile (read → lex → parse → resolve →
/// check → codegen → link). Created once per `build`/`run` and advanced with
/// `set_message` + `inc(1)`; hidden under `--quiet`. Bar and spinner render
/// in the brand green.
fn new_progress_bar(quiet: bool, total_steps: u64) -> ProgressBar {
    let pb = if quiet {
        ProgressBar::hidden()
    } else {
        ProgressBar::new(total_steps)
    };
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:30.green}] {pos}/{len} {msg}")
            .unwrap()
            .tick_strings(&[
                "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
            ])
            .progress_chars("#>-"),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Central error sink: all diagnostics render to stderr, then the process
/// exits non-zero.
fn fail(report: Report) -> ! {
    eprintln!("{report:?}");
    std::process::exit(1);
}

fn run_build(args: BuildArgs) -> miette::Result<()> {
    let opts = CompileOptions {
        file: &args.file,
        emit_llvm: args.emit_llvm,
        emit_llvm_file: args.emit_llvm_file.as_deref(),
        keep_obj: args.keep_obj,
        print_ast: args.print_ast,
        release: args.release,
        quiet: args.quiet,
        force_color: args.color,
        verbose: args.verbose,
        exe_path: args.output.clone(),
        check_only: false,
    };
    let _ = compile(opts)?;
    Ok(())
}

fn run_check(args: CheckArgs) -> miette::Result<()> {
    let opts = CompileOptions {
        file: &args.file,
        emit_llvm: false,
        emit_llvm_file: None,
        keep_obj: true,
        print_ast: args.print_ast,
        release: false,
        quiet: args.quiet,
        force_color: args.color,
        verbose: args.verbose,
        exe_path: None,
        check_only: true,
    };
    let _ = compile(opts)?;
    Ok(())
}

fn run_run(args: RunArgs) -> miette::Result<()> {
    // Temp binary next to the source (same filesystem → cheap, executable bit
    // preserved). Removed after the program finishes, even on failure.
    let stem = args
        .file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "holt-run".to_string());
    let tmp_exe = args.file.with_file_name(format!(
        ".{}-run-{}",
        stem,
        std::process::id()
    ));

    let opts = CompileOptions {
        file: &args.file,
        emit_llvm: false,
        emit_llvm_file: None,
        keep_obj: args.keep_obj,
        print_ast: args.print_ast,
        release: args.release,
        quiet: args.quiet,
        force_color: args.color,
        verbose: args.verbose,
        exe_path: Some(tmp_exe.clone()),
        check_only: false,
    };
    let built = compile(opts)?;
    let exe = match built {
        Some(p) => p,
        None => return Ok(()), // empty program: nothing to run
    };

    // ── Run ──────────────────────────────────────────────────────────
    if !args.quiet {
        eprintln!("{:>11} {}", brand("Running"), exe.display());
    }
    let status = Command::new(&exe)
        .args(&args.program_args)
        .status()
        .map_err(|e| {
            miette::miette!("failed to execute {}: {e}", exe.display())
        })?;
    // Always clean up the generated binary.
    let _ = fs::remove_file(&exe);

    // Propagate the program's exit code so `holt run` behaves like the binary.
    match status.code() {
        Some(0) | None => {
            if !status.success() {
                // Killed by signal (Unix): surface as failure.
                std::process::exit(1);
            }
            Ok(())
        }
        Some(code) => std::process::exit(code),
    }
}

/// Full pipeline: read → lex → parse → resolve → check → codegen/link.
///
/// A single progress bar covers the whole process, plus static status lines on
/// stderr. Returns the executable path when a binary was produced, or `None`
/// for `--emit-llvm` / empty programs.
fn compile(opts: CompileOptions<'_>) -> miette::Result<Option<PathBuf>> {
    let file = opts.file;
    init_colors(opts.force_color);
    let quiet = opts.quiet;
    let start_all = Instant::now();
    // read, lex, parse, resolve, check, [codegen/emit, link]
    let total_steps: u64 = if opts.check_only {
        5
    } else if opts.emit_llvm {
        6
    } else {
        7
    };
    let pb = new_progress_bar(quiet, total_steps);

        status(&pb, quiet, "Compiling", &gpath(file).to_string());

    // ── Read ─────────────────────────────────────────────────────────
    pb.set_message("Reading");
    let source = fs::read_to_string(file).map_err(|e| {
        pb.abandon();
        miette::miette!("failed to read {}: {e}", gpath(file))
    })?;
    let filename = file.display().to_string();
    pb.inc(1);
    if opts.verbose {
        status(&pb, quiet,
            "Reading",
            &format!("{} ({} bytes)", gpath(file), source.len()),
        );
    }

    // ── Lex ──────────────────────────────────────────────────────────
    pb.set_message("Lexing");
    let t_lex = Instant::now();
    let out = lex(&source);
    pb.inc(1);
    if opts.verbose {
        status(&pb, quiet,
            "Lexed",
            &format!(
                "{} tokens in {}",
                out.tokens.len(),
                gduration(t_lex.elapsed())
            ),
        );
    }
    if !out.errors.is_empty() {
        let parts: Vec<(compiler::token::Span, String)> = out
            .errors
            .into_iter()
            .map(|e| (e.span, format!("unexpected token `{}`", e.slice)))
            .collect();
        let multi = compiler::error::MultiDiagnostic::from_errors(
            filename.clone(),
            source.clone(),
            parts,
            "lexing failed".into(),
        );
        pb.abandon();
        fail(Report::new(multi));
    }
    let is_empty_program = out.tokens.iter().all(|st| {
        matches!(
            st.token,
            compiler::token::Token::Newline | compiler::token::Token::Semicolon
        )
    });
    if is_empty_program && out.tokens.is_empty() {
        pb.finish_with_message("Finished (empty file)");
        println!("ok: {} — 0 tokens (empty file)", filename);
        return Ok(None);
    }

    // ── Parse ────────────────────────────────────────────────────────
    pb.set_message("Parsing");
    let t_parse = Instant::now();
    let program =
        match compiler::parse::parse(out.tokens.clone(), source.clone()) {
            Ok(p) => {
                pb.inc(1);
                if opts.verbose {
                    status(&pb, quiet,
                        "Parsed",
                        &format!(
                            "{} items in {}",
                            p.items.len(),
                            gduration(t_parse.elapsed())
                        ),
                    );
                }
                p
            }
            Err(e) => {
                let diag = compiler::error::SingleDiagnostic::new(
                    filename.clone(),
                    source.clone(),
                    e.span,
                    e.message,
                );
                pb.abandon();
                fail(Report::new(diag));
            }
        };

    // ── Import expansion ─────────────────────────────────────────────
    pb.set_message("Resolving imports");
    let t_import = Instant::now();
    let (program, import_errors) = compiler::modules::expand_imports(program, file);
    if let Some(first) = import_errors.into_iter().next() {
        let diag = compiler::error::SingleDiagnostic::new(
            filename.clone(),
            source.clone(),
            first.span,
            first.message,
        );
        pb.abandon();
        fail(Report::new(diag));
    }
    let program = {
        pb.inc(1);
        if opts.verbose {
            status(&pb, quiet,
                "Resolved",
                &format!(
                    "{} items in {}",
                    program.items.len(),
                    gduration(t_import.elapsed())
                ),
            );
        }
        program
    };
    if opts.print_ast {
        println!("{:#?}", program);
    }

    // ── Sema ─────────────────────────────────────────────────────────
    pb.set_message("Checking");
    status(&pb, quiet, "Checking", &filename);
    let t_check = Instant::now();
    let sema_errors = compiler::sema::check(&program);
    pb.inc(1);
    if !sema_errors.is_empty() {
        let parts: Vec<(compiler::token::Span, String)> = sema_errors
            .into_iter()
            .map(|e| (e.span, e.message))
            .collect();
        let multi = compiler::error::MultiDiagnostic::from_errors(
            filename.clone(),
            source.clone(),
            parts,
            "semantic error".into(),
        );
        pb.abandon();
                fail(Report::new(multi));
    }
    if opts.check_only {
        pb.finish_with_message("Finished");
        status(&pb, quiet,
            "Checked",
            &format!(
                "{} in {}",
                gpath(file),
                gduration(start_all.elapsed())
            ),
        );
        return Ok(None);
    }
    status(&pb, quiet,
        "Checked",
        &format!("in {}", seconds(t_check.elapsed())),
    );

    let opt = if opts.release {
        compiler::codegen::OptLevel::Release
    } else {
        compiler::codegen::OptLevel::Debug
    };

        // ── Codegen ──────────────────────────────────────────────────────
    if opts.emit_llvm {
        pb.set_message("Generating LLVM IR");
        status(&pb, quiet, "Generating", "LLVM IR");
        let t_ir = Instant::now();
        let ir = match generate_ir_string(&program, opt) {
            Ok(ir) => ir,
            Err(e) => {
                pb.abandon();
                return Err(e);
            }
        };
        pb.inc(1);
        status(&pb, quiet,
            "Generated",
            &format!("in {}", gduration(t_ir.elapsed())),
        );
        if let Some(path) = opts.emit_llvm_file {
            fs::write(path, &ir).map_err(|e| miette::miette!("failed to write IR: {e}"))?;
            status(&pb, quiet, "Exported", &gpath(path).to_string());
        } else {
            println!("{ir}");
        }
        pb.finish_with_message("Finished");
        status(&pb, quiet,
            "Compiled",
            &format!("in {}", gduration(start_all.elapsed())),
        );
        return Ok(None);
    }

    let exe_path = default_exe_path(file, opts.exe_path.clone());
    let obj_path = obj_path_for_exe(&exe_path);

    pb.set_message(format!("Compiling {}", obj_path.display()));
    let build_kind = if opts.release { "release" } else { "debug" };
    status(&pb, quiet, "Compiling", &format!("{} ({build_kind})", gpath(&obj_path)));
    let t_cg = Instant::now();
    if let Err(e) = codegen_to_object(&program, &obj_path, &filename, &source, opt) {
        pb.abandon();
        return Err(e);
    }
    pb.inc(1);
    if opts.verbose {
        status(&pb, quiet,
            "Compiled",
            &format!("{} in {}", gpath(&obj_path), gduration(t_cg.elapsed())),
        );
    }

            // ── Link ─────────────────────────────────────────────────────────
    pb.set_message(format!("Linking {}", exe_path.display()));
    let t_link = Instant::now();
    let link_status = Command::new("clang")
        .arg(&obj_path)
        .arg("-o")
        .arg(&exe_path)
        .status()
        .map_err(|e| {
            pb.abandon();
            miette::miette!("failed to invoke clang: {e} — is Xcode CLT installed?")
        })?;
    if !link_status.success() {
        pb.abandon();
        return Err(miette::miette!("linking failed with clang"));
    }
    pb.inc(1);
    if opts.verbose {
        status(&pb, quiet,
            "Linked",
            &format!("{} in {}", gpath(&exe_path), gduration(t_link.elapsed())),
        );
    }

    if !opts.keep_obj {
        let _ = fs::remove_file(&obj_path);
    }

    pb.finish_with_message("Finished");
    status(&pb, quiet,
        "Compiled",
        &format!(
            "{} → {} in {}",
            gpath(file),
            gpath(&exe_path),
            gduration(start_all.elapsed())
        ),
    );
    Ok(Some(exe_path))
}

fn generate_ir_string(
    program: &compiler::ast::Program,
    opt: compiler::codegen::OptLevel,
) -> miette::Result<String> {
    use inkwell::context::Context;
    let ctx = Context::create();
    let mut cg = compiler::codegen::Codegen::new(&ctx, "holt");
    cg.compile_program(program).map_err(|e| {
        miette::miette!(
            "codegen error: {} at {}..{}",
            e.message,
            e.span.start,
            e.span.end
        )
    })?;
    if opt == compiler::codegen::OptLevel::Release {
        let machine = compiler::codegen::target_machine(opt)
            .map_err(|e| miette::miette!("failed to create target machine: {e}"))?;
        cg.optimize_for_release(&machine)
            .map_err(|e| miette::miette!("release passes failed: {e}"))?;
    }
    Ok(cg.get_module_ir())
}

fn codegen_to_object(
    program: &compiler::ast::Program,
    obj_path: &Path,
    filename: &str,
    source: &str,
    opt: compiler::codegen::OptLevel,
) -> miette::Result<()> {
    if let Err(msg) = compiler::codegen::compile_to_object(program, obj_path, opt) {
        let diag = compiler::error::SingleDiagnostic::new(
            filename.to_string(),
            source.to_string(),
            compiler::token::Span::new(0, source.len()),
            msg,
        );
        // Render to stderr via the single diagnostic sink, then exit non-zero.
        fail(Report::new(diag));
    }
    Ok(())
}

/// Default executable path: the input file with its extension stripped
/// (`examples/hello.hlt` → `examples/hello`), i.e. a proper binary with no
/// `.out` suffix. An explicit `-o/--output` is used verbatim.
fn default_exe_path(input: &Path, override_: Option<PathBuf>) -> PathBuf {
    if let Some(o) = override_ {
        return o;
    }
    let mut p = input.to_path_buf();
    // Strip only a real extension (e.g. `.hlt`); extensionless input stays as-is.
    if input.extension().is_some() {
        p.set_extension("");
    }
    p
}

/// Object path derived from the executable path, guaranteed not to collide
/// with the exe itself (e.g. `-o foo.o` → `foo.obj.o` instead of `foo.o`).
fn obj_path_for_exe(exe: &Path) -> PathBuf {
    if exe.extension().is_some_and(|e| e == "o") {
        let mut p = exe.to_path_buf();
        p.set_extension("obj.o");
        p
    } else {
        let mut p = exe.to_path_buf();
        p.set_extension("o");
        p
    }
}

