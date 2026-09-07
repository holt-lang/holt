mod ast;
mod codegen;
mod error;
mod lexer;
mod parse;
mod sema;
mod token;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use miette::Report;

use lexer::lex;

/// Holt compiler driver (Phase 0–1). Lexing always happens; with --lex we stop after tokens.
#[derive(Parser, Debug)]
#[command(name = "holtc", about = "Holt compiler (Phase 1 — MVP executable)")]
struct Args {
    /// Source file (.hlt) to compile
    file: PathBuf,

    /// Only lex and print tokens (old Phase 0 behavior)
    #[arg(long, default_value_t = false)]
    lex: bool,

    /// Show token spans as byte offsets (lex mode)
    #[arg(long, default_value_t = false)]
    show_spans: bool,

    /// Emit LLVM IR to stdout and exit (no link)
    #[arg(long, default_value_t = false)]
    emit_llvm: bool,

    /// When --emit-llvm, write IR to file instead of stdout
    #[arg(long)]
    emit_llvm_file: Option<PathBuf>,

    /// Keep object file (don't delete after linking)
    #[arg(long, default_value_t = false)]
    keep_obj: bool,

    /// Output executable path (default: <input>.out)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Print AST for debugging
    #[arg(long, default_value_t = false)]
    print_ast: bool,
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    let source = fs::read_to_string(&args.file).map_err(|e| {
        miette::miette!("failed to read {}: {e}", args.file.display())
    })?;
    let filename = args.file.display().to_string();

    // ── Lex ─────────────────────────────────────────────────────────
    let out = lex(&source);
    if !out.errors.is_empty() {
        let parts: Vec<(crate::token::Span, String)> = out
            .errors
            .into_iter()
            .map(|e| (e.span, format!("unexpected token `{}`", e.slice)))
            .collect();
        let multi = error::MultiDiagnostic::from_errors(
            filename.clone(),
            source.clone(),
            parts,
            "lexing failed".into(),
        );
        eprintln!("{:?}", Report::new(multi));
        std::process::exit(1);
    }

    // Phase 0 compatibility: empty file (only trivia/newlines) is lex-ok even when compiling
    let is_empty_program = out.tokens.iter().all(|st| {
        matches!(st.token, token::Token::Newline | token::Token::Semicolon)
    });
    if is_empty_program {
        if args.lex {
            println!("ok: {} — 0 tokens (empty file)", filename);
            return Ok(());
        } else {
            // When compiling empty file, still succeed as lex (Phase 0 exit criteria: cargo run -- examples/empty.hlt)
            if out.tokens.is_empty() {
                println!("ok: {} — 0 tokens (empty file)", filename);
                return Ok(());
            }
            // If file only contains newlines/semicolons, also treat as empty
        }
    }

    if args.lex {
        for st in &out.tokens {
            let slice = &source[st.span.start..st.span.end];
            let display_slice = if st.token == token::Token::Newline {
                "\\n".to_string()
            } else {
                slice.replace('\n', "\\n")
            };
            if args.show_spans {
                println!(
                    "{:>4}..{:<4}  {:18}  `{}`",
                    st.span.start,
                    st.span.end,
                    format!("{}", st.token),
                    display_slice
                );
            } else {
                println!(
                    "{:18}  `{}`  @{}..{}",
                    format!("{}", st.token),
                    display_slice,
                    st.span.start,
                    st.span.end
                );
            }
        }
        println!("ok: {} — {} tokens, 0 errors", filename, out.tokens.len());
        return Ok(());
    }

    // ── Parse ───────────────────────────────────────────────────────
    let program = match parse::parse(out.tokens.clone(), source.clone()) {
        Ok(p) => p,
        Err(e) => {
            let diag = error::SingleDiagnostic::new(
                filename.clone(),
                source.clone(),
                e.span,
                e.message,
            );
            eprintln!("{:?}", Report::new(diag));
            std::process::exit(1);
        }
    };

    if args.print_ast {
        println!("{:#?}", program);
    }

    // ── Sema ────────────────────────────────────────────────────────
    let sema_errors = sema::check(&program);
    if !sema_errors.is_empty() {
        let parts: Vec<(crate::token::Span, String)> = sema_errors
            .into_iter()
            .map(|e| (e.span, e.message))
            .collect();
        let multi = error::MultiDiagnostic::from_errors(
            filename.clone(),
            source.clone(),
            parts,
            "semantic error".into(),
        );
        eprintln!("{:?}", Report::new(multi));
        std::process::exit(1);
    }

    // ── Codegen (object) ────────────────────────────────────────────
    // Temporary object file
    let obj_path = if let Some(o) = &args.output {
        // if output is foo, obj is foo.o
        let mut p = o.clone();
        p.set_extension("o");
        p
    } else {
        let mut p = args.file.clone();
        p.set_extension("o");
        p
    };

    // If emit_llvm requested, print IR and exit (no link unless also output?)
    if args.emit_llvm {
        let ir = generate_ir_string(&program)?;
        if let Some(path) = &args.emit_llvm_file {
            fs::write(path, &ir)
                .map_err(|e| miette::miette!("failed to write IR: {e}"))?;
            eprintln!("wrote IR to {}", path.display());
        } else {
            println!("{ir}");
        }
        return Ok(());
    }

    // Compile to object using inkwell TargetMachine
    codegen_to_object(&program, &obj_path, &filename, &source)?;

    // ── Link with clang ─────────────────────────────────────────────
    let exe_path = args.output.clone().unwrap_or_else(|| {
        let mut p = args.file.clone();
        p.set_extension("");
        let name =
            p.file_name().unwrap().to_string_lossy().to_string() + ".out";
        p.with_file_name(name)
    });

    let status = Command::new("clang")
        .arg(&obj_path)
        .arg("-o")
        .arg(&exe_path)
        .status()
        .map_err(|e| {
            miette::miette!(
                "failed to invoke clang: {e} — is Xcode CLT installed?"
            )
        })?;

    if !status.success() {
        return Err(miette::miette!("linking failed with clang"));
    }

    if !args.keep_obj {
        let _ = fs::remove_file(&obj_path);
    }

    eprintln!(
        "ok: {} → {} (linked)",
        args.file.display(),
        exe_path.display()
    );
    Ok(())
}

fn generate_ir_string(program: &ast::Program) -> miette::Result<String> {
    use inkwell::context::Context;
    let ctx = Context::create();
    let mut cg = codegen::Codegen::new(&ctx, "holt");
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
    program: &ast::Program,
    obj_path: &Path,
    filename: &str,
    source: &str,
) -> miette::Result<()> {
    if let Err(msg) = codegen::compile_to_object(program, obj_path) {
        let diag = error::SingleDiagnostic::new(
            filename.to_string(),
            source.to_string(),
            crate::token::Span::new(0, source.len()),
            msg,
        );
        eprintln!("{:?}", Report::new(diag));
        std::process::exit(1);
    }
    Ok(())
}
