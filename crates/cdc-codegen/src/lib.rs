//! Backend de compilation native de cadernislang.
//!
//! Le backend LLVM (via `inkwell`, LLVM 18 — ADR-002) est **optionnel** : compilé seulement avec
//! la feature `llvm`. Sans elle, `cdc run`/`cdc check` fonctionnent normalement (exigence SPEC),
//! et `cdc build` renvoie une erreur explicite. Cela garde le workspace buildable sans LLVM.

use cdc_ast::Program;
use std::path::Path;

#[cfg(feature = "llvm")]
mod llvm;

/// Compile `program` en binaire natif `out`, en liant la staticlib `cdc-runtime` (`runtime_lib`).
///
/// # Erreurs
/// Erreur de génération/link, ou (sans la feature `llvm`) backend indisponible.
#[cfg(feature = "llvm")]
pub fn build(program: &Program, out: &Path, runtime_lib: &Path) -> Result<(), String> {
    llvm::build(program, out, runtime_lib)
}

/// Variante sans backend LLVM compilé.
///
/// # Erreurs
/// Toujours : il faut recompiler avec `--features llvm`.
#[cfg(not(feature = "llvm"))]
pub fn build(_program: &Program, _out: &Path, _runtime_lib: &Path) -> Result<(), String> {
    Err("backend LLVM non compilé — recompiler avec « --features llvm » (LLVM 18 + \
         LLVM_SYS_180_PREFIX requis, voir ADR-002)"
        .to_string())
}
