use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use miette::Report;

use compiler::lexer::lex;

/// Holt — main entry point (wraps `compiler` crate)
#[derive(Parser, Debug)]
#[command(name = "holt", version = "0.1.0", about = "Holt compiler — build with progress like cargo")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build a .hlt source file (lex → parse → check → codegen → link)
    Build(BuildArgs),
    /// Build and run a .hlt source file, removing the binary afterwards
    Run(RunArgs),
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

    /// Show verbose progress (default: single progress bar + summary lines)
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

    /// Show verbose progress (default: single progress bar + summary lines)
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Arguments forwarded to the program (use `--` to separate them)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    program_args: Vec<String>,
}

/// Shared knobs for the compile pipeline (used by both `build` and `run`).
struct CompileOptions<'a> {
    file: &'a Path,
    emit_llvm: bool,
    emit_llvm_file: Option<&'a Path>,
    keep_obj: bool,
    print_ast: bool,
    verbose: bool,
    exe_path: Option<PathBuf>,
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build(args) => run_build(args),
        Commands::Run(args) => run_run(args),
    }
}

/// Single progress bar for the whole compile pipeline.
///
/// Created once per `build`/`run` invocation and advanced with
/// `set_message` + `inc(1)` as each phase completes.
fn new_progress_bar(total_steps: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_steps);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} {msg}",
        )
        .unwrap()
        .tick_strings(&[
            "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
        ])
        .progress_chars("#>-"),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

fn fail_progress(pb: &ProgressBar, msg: &str) {
    pb.abandon_with_message(msg.to_owned());
}

fn run_build(args: BuildArgs) -> miette::Result<()> {
    let opts = CompileOptions {
        file: &args.file,
        emit_llvm: args.emit_llvm,
        emit_llvm_file: args.emit_llvm_file.as_deref(),
        keep_obj: args.keep_obj,
        print_ast: args.print_ast,
        verbose: args.verbose,
        exe_path: args.output.clone(),
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
        verbose: args.verbose,
        exe_path: Some(tmp_exe.clone()),
    };
    let built = compile(opts)?;
    let exe = match built {
        Some(p) => p,
        None => return Ok(()), // empty program: nothing to run
    };

    // ── Run ──────────────────────────────────────────────────────────
    eprintln!(
        "{:>12} {}",
        style("Running").green().bold(),
        exe.display()
    );
    let status = Command::new(&exe)
        .args(&args.program_args)
        .status()
        .map_err(|e| miette::miette!("failed to execute {}: {e}", exe.display()))?;
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
/// Uses ONE progress bar for the whole process. Returns the executable path
/// when a binary was produced, or `None` for `--emit-llvm` / empty programs.
fn compile(opts: CompileOptions<'_>) -> miette::Result<Option<PathBuf>> {
    let file = opts.file;
    let start_all = Instant::now();
    // read, lex, parse, resolve, check, codegen/emit, link
    let total_steps: u64 = if opts.emit_llvm { 6 } else { 7 };
    let pb = new_progress_bar(total_steps);

    // Header like cargo: Compiling holt v0.1.0 (file)
    eprintln!(
        "{:>12} {} v{} ({})",
        style("Compiling").green().bold(),
        style("holt").bold(),
        env!("CARGO_PKG_VERSION"),
        file.display()
    );

    // ── Read ─────────────────────────────────────────────────────────
    pb.set_message(format!("Reading {}", file.display()));
    let source = fs::read_to_string(file).map_err(|e| {
        fail_progress(&pb, "Reading failed");
        miette::miette!("failed to read {}: {e}", file.display())
    })?;
    let filename = file.display().to_string();
    pb.inc(1);
    if opts.verbose {
        eprintln!("{:>12} {} ({} bytes)", style("Reading").green().bold(), file.display(), source.len());
    }

    // ── Lex ──────────────────────────────────────────────────────────
    pb.set_message("Lexing");
    let t_lex = Instant::now();
    let out = lex(&source);
    pb.inc(1);
    eprintln!(
        "{:>12} {} ({} tokens, {:?})",
        style("Lexing").green().bold(),
        file.display(),
        out.tokens.len(),
        t_lex.elapsed()
    );
    if !out.errors.is_empty() {
        fail_progress(&pb, "Lexing failed");
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
        eprintln!("{:?}", Report::new(multi));
        std::process::exit(1);
    }
    let is_empty_program = out.tokens.iter().all(|st| {
        matches!(st.token, compiler::token::Token::Newline | compiler::token::Token::Semicolon)
    });
    if is_empty_program && out.tokens.is_empty() {
        pb.finish_with_message("Finished (empty file)");
        eprintln!("{:>12} empty file", style("Finished").green().bold());
        println!("ok: {} — 0 tokens (empty file)", filename);
        return Ok(None);
    }

    // ── Parse ────────────────────────────────────────────────────────
    pb.set_message("Parsing");
    let t_parse = Instant::now();
    let program = match compiler::parse::parse(out.tokens.clone(), source.clone()) {
        Ok(p) => {
            pb.inc(1);
            eprintln!(
                "{:>12} ({} items, {:?})",
                style("Parsing").green().bold(),
                p.items.len(),
                t_parse.elapsed()
            );
            p
        }
        Err(e) => {
            fail_progress(&pb, "Parsing failed");
            let diag = compiler::error::SingleDiagnostic::new(
                filename.clone(),
                source.clone(),
                e.span,
                e.message,
            );
            eprintln!("{:?}", Report::new(diag));
            std::process::exit(1);
        }
    };

    // ── Import expansion ─────────────────────────────────────────────
    pb.set_message("Resolving imports");
    let t_import = Instant::now();
    let program = match expand_imports(program, file) {
        Ok(p) => {
            pb.inc(1);
            eprintln!(
                "{:>12} ({} items, {:?})",
                style("Resolving").green().bold(),
                p.items.len(),
                t_import.elapsed()
            );
            p
        }
        Err(e) => {
            fail_progress(&pb, "Resolving failed");
            let diag = compiler::error::SingleDiagnostic::new(
                filename.clone(),
                source.clone(),
                e.span,
                e.message,
            );
            eprintln!("{:?}", Report::new(diag));
            std::process::exit(1);
        }
    };
    if opts.print_ast {
        println!("{:#?}", program);
    }

    // ── Sema ─────────────────────────────────────────────────────────
    pb.set_message("Checking");
    let t_check = Instant::now();
    let sema_errors = compiler::sema::check(&program);
    pb.inc(1);
    if !sema_errors.is_empty() {
        fail_progress(&pb, "Checking failed");
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
        eprintln!("{:?}", Report::new(multi));
        std::process::exit(1);
    }
    eprintln!(
        "{:>12} ({:?})",
        style("Checking").green().bold(),
        t_check.elapsed()
    );

    // ── Codegen ──────────────────────────────────────────────────────
    if opts.emit_llvm {
        pb.set_message("Emitting LLVM IR");
        let t_ir = Instant::now();
        let ir = match generate_ir_string(&program) {
            Ok(ir) => ir,
            Err(e) => {
                fail_progress(&pb, "Emitting failed");
                return Err(e);
            }
        };
        pb.inc(1);
        eprintln!(
            "{:>12} ({:?})",
            style("Emitting").green().bold(),
            t_ir.elapsed()
        );
        if let Some(path) = opts.emit_llvm_file {
            fs::write(path, &ir).map_err(|e| miette::miette!("failed to write IR: {e}"))?;
            eprintln!("{:>12} {}", style("Wrote").green().bold(), path.display());
        } else {
            println!("{ir}");
        }
        pb.finish_with_message("Finished");
        eprintln!(
            "{:>12} in {:?}",
            style("Finished").green().bold(),
            start_all.elapsed()
        );
        return Ok(None);
    }

    let exe_path = default_exe_path(file, opts.exe_path.clone());
    let obj_path = obj_path_for_exe(&exe_path);

    pb.set_message("Codegen");
    let t_cg = Instant::now();
    codegen_to_object(&program, &obj_path, &filename, &source).map_err(|e| {
        fail_progress(&pb, "Codegen failed");
        e
    })?;
    pb.inc(1);
    eprintln!(
        "{:>12} {} ({:?})",
        style("Codegen").green().bold(),
        obj_path.display(),
        t_cg.elapsed()
    );

    // ── Link ─────────────────────────────────────────────────────────
    pb.set_message(format!("Linking {}", exe_path.display()));
    let t_link = Instant::now();
    let status = Command::new("clang")
        .arg(&obj_path)
        .arg("-o")
        .arg(&exe_path)
        .status()
        .map_err(|e| {
            fail_progress(&pb, "Linking failed");
            miette::miette!("failed to invoke clang: {e} — is Xcode CLT installed?")
        })?;
    if !status.success() {
        fail_progress(&pb, "Linking failed");
        return Err(miette::miette!("linking failed with clang"));
    }
    pb.inc(1);
    eprintln!(
        "{:>12} {} ({:?})",
        style("Linking").green().bold(),
        exe_path.display(),
        t_link.elapsed()
    );

    if !opts.keep_obj {
        let _ = fs::remove_file(&obj_path);
    }

    pb.finish_with_message("Finished");
    eprintln!(
        "{:>12} {} → {} in {:?}",
        style("Finished").green().bold(),
        file.display(),
        exe_path.display(),
        start_all.elapsed()
    );
    Ok(Some(exe_path))
}

fn generate_ir_string(program: &compiler::ast::Program) -> miette::Result<String> {
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
    Ok(cg.get_module_ir())
}

fn codegen_to_object(
    program: &compiler::ast::Program,
    obj_path: &Path,
    filename: &str,
    source: &str,
) -> miette::Result<()> {
    if let Err(msg) = compiler::codegen::compile_to_object(program, obj_path) {
        let diag = compiler::error::SingleDiagnostic::new(
            filename.to_string(),
            source.to_string(),
            compiler::token::Span::new(0, source.len()),
            msg,
        );
        eprintln!("{:?}", Report::new(diag));
        // Return an error (instead of exiting here) so the caller can
        // abandon the shared progress bar before exiting.
        return Err(miette::miette!("codegen failed"));
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

fn find_stdlib_root(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    };
    loop {
        let candidate = dir.join("stdlib");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    let cwd = PathBuf::from("stdlib");
    if cwd.is_dir() {
        return Some(cwd);
    }
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../stdlib");
    if ws.is_dir() {
        return Some(ws);
    }
    None
}

fn expand_imports(
    mut program: compiler::ast::Program,
    importer: &Path,
) -> Result<compiler::ast::Program, compiler::parse::ParseError> {
    use std::collections::HashSet;
    let stdlib_root = find_stdlib_root(importer);
    let mut visited: HashSet<String> = HashSet::new();
    let mut out_items: Vec<compiler::ast::Item> = Vec::new();
    let mut pending: Vec<compiler::ast::Item> = std::mem::take(&mut program.items);
    let mut idx = 0;
    while idx < pending.len() {
        let item = pending[idx].clone();
        if let compiler::ast::Item::Import(imp) = item {
            let key = imp.path.join("::");
            if visited.contains(&key) {
                idx += 1;
                continue;
            }
            visited.insert(key.clone());
            let rel = imp.path.join("/") + ".hlt";
            let mut file_path: Option<PathBuf> = None;
            if let Some(ref root) = stdlib_root {
                let p = root.join(&rel);
                if p.is_file() {
                    file_path = Some(p);
                }
            }
            if file_path.is_none() {
                if let Some(parent) = importer.parent() {
                    let p = parent.join(&rel);
                    if p.is_file() {
                        file_path = Some(p);
                    }
                }
            }
            if file_path.is_none() {
                let p = PathBuf::from(&rel);
                if p.is_file() {
                    file_path = Some(p);
                }
            }
            let path = file_path.ok_or_else(|| compiler::parse::ParseError {
                message: format!("cannot resolve import `{}`", imp.path.join("::")),
                span: imp.span,
            })?;
            let src = fs::read_to_string(&path).map_err(|e| compiler::parse::ParseError {
                message: format!("failed to read import {}: {e}", path.display()),
                span: imp.span,
            })?;
            let toks = compiler::lexer::lex(&src);
            if !toks.errors.is_empty() {
                return Err(compiler::parse::ParseError {
                    message: format!("lex error in import {}", path.display()),
                    span: imp.span,
                });
            }
            let mut sub = compiler::parse::parse(toks.tokens, src.clone()).map_err(|e| compiler::parse::ParseError {
                message: format!("parse error in {}: {}", path.display(), e.message),
                span: imp.span,
            })?;
            sub = expand_imports(sub, &path)?;
            if let Some(ref syms) = imp.symbols {
                let wanted: HashSet<String> = syms.iter().map(|(s, _)| s.clone()).collect();
                for it in sub.items {
                    match &it {
                        compiler::ast::Item::Function(f) if wanted.contains(&f.name) => out_items.push(it),
                        compiler::ast::Item::Struct(s) if wanted.contains(&s.name) => out_items.push(it),
                        compiler::ast::Item::Class(c) if wanted.contains(&c.name) => out_items.push(it),
                        compiler::ast::Item::Enum(e) if wanted.contains(&e.name) => out_items.push(it),
                        compiler::ast::Item::Import(_) => {}
                        _ => {}
                    }
                }
            } else {
                for it in sub.items.into_iter().filter(|i| !matches!(i, compiler::ast::Item::Import(_))) {
                    out_items.push(it);
                }
            }
        } else {
            out_items.push(item);
        }
        idx += 1;
    }
    Ok(compiler::ast::Program {
        items: out_items,
        span: program.span,
    })
}
