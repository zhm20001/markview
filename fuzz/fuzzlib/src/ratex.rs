//! A documented allowance for one upstream `ratex-parser` defect.
//!
//! `ratex-parser` 0.1.14 accumulates the `\char` argument into an `i64` at
//! `macro_expander.rs:823` (`number = number * (b as i64) + d;`). A literal
//! wide enough to overflow panics only when overflow checks are on: this
//! build has them (`cargo-fuzz` passes `-Cdebug-assertions`), while the
//! reader's release profile does not, and there the multiply wraps into a
//! negative `\@char` code point that then fails to parse. So no reader
//! observes this.
//!
//! It is still fatal to a campaign. `libfuzzer-sys` installs a panic hook
//! that aborts before unwinding, so the `catch_unwind` in
//! `markview_core::math` cannot absorb the panic and libFuzzer reports a
//! crash. Replacing that hook with one that lets exactly this panic unwind
//! puts `catch_unwind` back in service; every other panic still aborts, so
//! the oracle stays strict.
//!
//! The class reaches `math`, `layout`, `layout_diff`, and `pdf`. Delete this
//! module and its call sites once the dependency is fixed or bumped.

use std::sync::Once;

/// Whether a panic from `file` with `message` is that upstream overflow,
/// which `catch_unwind` should be allowed to absorb. Scoped to the one
/// location and message, so any other `ratex` panic still aborts.
fn is_char_overflow(file: &str, message: &str) -> bool {
	file.contains("ratex-parser")
		&& file.ends_with("macro_expander.rs")
		&& message.contains("attempt to multiply with overflow")
}

/// Installs the allowance, once per process, before the first input.
pub fn allow_char_overflow() {
	static ONCE: Once = Once::new();
	ONCE.call_once(|| {
		// Keep libFuzzer's hook — it prints and aborts — for every panic the
		// allowance does not name.
		let abort_on_panic = std::panic::take_hook();
		std::panic::set_hook(Box::new(move |info| {
			let file = info.location().map_or("", |l| l.file());
			let message = info.payload_as_str().unwrap_or("");
			if !is_char_overflow(file, message) {
				abort_on_panic(info);
			}
		}));
	});
}

#[cfg(test)]
mod tests {
	use super::is_char_overflow;

	#[test]
	fn only_the_upstream_multiply_is_absorbed() {
		let upstream = "/x/ratex-parser-0.1.14/src/macro_expander.rs";
		assert!(is_char_overflow(
			upstream,
			"attempt to multiply with overflow"
		));
		// A different function, crate, or panic must still abort.
		assert!(!is_char_overflow(upstream, "index out of bounds"));
		assert!(!is_char_overflow(
			"/x/ratex-parser-0.1.14/src/parser.rs",
			"attempt to multiply with overflow"
		));
		assert!(!is_char_overflow(
			"/x/parley-0.11.1/src/layout/data.rs",
			"attempt to multiply with overflow"
		));
	}
}
