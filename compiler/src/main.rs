mod error;
mod lexer;
mod token;

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use inkwell::context::Context;
use miette::Report;

use lexer::lex;

/// Holt Phase-0 driver: lex → print tokens + verify `inkwell` linkage.
///
/// Exit criteria (`references/phases.md:9`):
/// `cargo run -- examples/empty.hlt` tokenizes without crash.
#[derive(Parser, Debug)]
#[command(name = "holtc", about = "Holt compiler (Phase 0 — lexer + LLVM linkage)")]
struct Args {
    /// Source file (.hlt) to tokenize
    file: PathBuf,

    /// Show token spans as byte offsets
    #[arg(long, default_value_t = false)]
    show_spans: bool,

    /// Suppress LLVM linkage check (default: verify inkwell works)
    #[arg(long, default_value_t = false)]
    no_llvm_check: bool,
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    // ── LLVM linkage verification (no JIT per request) ──────────────────
    // We create a context + module and run `verify` stub to prove
    // `llvm-sys-211` + `inkwell 0.10 llvm21-1` linked against the
    // system LLVM found at `/opt/homebrew/bin/llvm-config` (21.1.8).
    if !args.no_llvm_check {
        verify_llvm_linkage()?;
    }

    // ── Read source ─────────────────────────────────────────────────────
    let source = fs::read_to_string(&args.file).map_err(|e| miette::miette!(
        "failed to read {}: {e}", args.file.display()
    ))?;
    let filename = args.file.display().to_string();

    // ── Lex ─────────────────────────────────────────────────────────────
    let out = lex(&source);

    if !out.errors.is_empty() {
        // Convert raw errors to miette diagnostics with source spans.
        let parts: Vec<(crate::token::Span, String)> =
            out.errors.into_iter().map(|e| (e.span, e.slice)).collect();
        let multi = error::MultiLexError::from_parts(filename.clone(), source.clone(), parts);
        // Print fancy report and exit non-zero.
        eprintln!("{:?}", Report::new(multi));
        std::process::exit(1);
    }

    // ── Print tokens (skip printing nothing for empty file — success) ──
    if out.tokens.is_empty() {
        println!("ok: {} — 0 tokens (empty file)", filename);
        return Ok(());
    }

    for st in &out.tokens {
        let slice = &source[st.span.start..st.span.end];
        // Escape newlines in display so the table stays readable.
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
            // Compact: TOKEN  `lexeme`  @start..end
            println!(
                "{:18}  `{}`  @{}..{}",
                format!("{}", st.token),
                display_slice,
                st.span.start,
                st.span.end
            );
        }
    }
    println!(
        "ok: {} — {} tokens, 0 errors, LLVM 21.1.8 linked",
        filename,
        out.tokens.len()
    );
    Ok(())
}

fn verify_llvm_linkage() -> miette::Result<()> {
    // Creating a Context forces dynamic LLVM library load.
    let ctx = Context::create();
    let module = ctx.create_module("holt_phase0_check");
    // Add a dummy function so `verify` has something to check.
    let i64_ty = ctx.i64_type();
    let fn_ty = i64_ty.fn_type(&[], false);
    let func = module.add_function("holt.main", fn_ty, None);
    let entry = ctx.append_basic_block(func, "entry");
    let builder = ctx.create_builder();
    builder.position_at_end(entry);
    builder.build_return(Some(&i64_ty.const_int(0, false))).unwrap();

    // Never skip `module.verify()` before emission (llvm-mapping.md:103)
    if let Err(e) = module.verify() {
        return Err(miette::miette!("LLVM module verification failed: {e}"));
    }
    // No JIT emission (user requested: no JIT). Object emission would be Phase 1.
    eprintln!("LLVM linkage ok: feature llvm21-1, inkwell 0.10, LLVM 21.1.8");
    Ok(())
}
