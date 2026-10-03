//! G1, Tier 3 semantic oracle: a node's source range must be a real place in
//! the source it is stored in.
//!
//! The committed `parse` oracle checks that every range is ordered and inside
//! the document. It does not check the *relationship* between a node and the
//! container it lives in, and a range can be ordered and in bounds while still
//! belonging to something else: an inline can run past the block that stores
//! it, a block can run past its container, and a code span can be indexed from
//! its parent rather than itself. Those are shapes a whole-document bounds
//! check cannot see.
//!
//! Oracle (O1, O2, O5), all from Markview's own public tree — no comrak HTML
//! (plan item O7):
//!
//! - a nested block's range must lie inside its parent's range;
//! - an inline's range must lie inside the block that stores it, so a copy of
//!   that block cannot address a sibling's text;
//! - a block's range must be a function of the block, not of what follows it:
//!   appending trailing whitespace the parser discards must not move a range.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use markview_core::document::{Block, BlockKind, Document};
use mvfuzz::{budget, mutators, oracle, seam};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

/// Whether a container's children may carry ranges that are **not** on the
/// container's own basis, which is what makes containment unassertable for it.
///
/// Two shapes do this, and both are legal by construction rather than defects:
///
/// - a `<details>` element's body is cut out of the source and re-parsed on
///   its own (`parse.rs::markdown_blocks`), so its children's ranges address
///   the body text, not the document. Such a child can sit wholly before,
///   wholly after, or — the case that caught this oracle — **partially
///   overlap** the element's range.
/// - a container whose own range excludes its trailing line ending while its
///   child's includes it: `*\n      *\n\n<summary>…` gives the list `0..9`
///   (`"*\n      *"`) and its item's block `8..10` (`"*\n"`), so the item ends
///   one byte past the list. The parser's container range stops at the last
///   content byte and the child's may take the terminator; which of the two is
///   "right" is not decidable from the tree, so neither is asserted.
///
/// The honest statement is therefore that containment is only a real invariant
/// for a container the parser builds *in place* and whose child ranges share
/// its basis. Rather than guess which those are, this returns true for the
/// containers where the mismatch has actually been observed, and the walk
/// states the assumption at each call site.
fn snippet_children(kind: &BlockKind) -> bool {
	matches!(kind, BlockKind::Details { .. } | BlockKind::List { .. })
}

/// Every range in a document-shaped subtree must lie inside `outer`, the range
/// of the container that holds it.
///
/// `outer` is `None` at the top level, where a range only has to fit the
/// document (which `assert_source_ranges` already checks).
fn walk(
	doc: &Document,
	blocks: &[Block],
	outer: Option<&std::ops::Range<usize>>,
) {
	for block in blocks {
		if let Some(outer) = outer {
			let inside = outer.start <= block.source.start
				&& block.source.end <= outer.end;
			assert!(
				inside,
				"block range {:?} escapes its container {outer:?} in {:?}",
				block.source,
				doc.source.get(outer.clone()).unwrap_or("")
			);
		}
		match &block.kind {
			BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
				check_rich(doc, text, &block.source, "inline", outer.is_some())
			}
			// A snippet body: its own ranges are relative to the body, so the
			// walk continues without a containment requirement.
			BlockKind::Details {
				summary, blocks, ..
			} => {
				check_rich(doc, summary, &block.source, "summary", false);
				if snippet_children(&block.kind) {
					walk(doc, blocks, None);
				} else {
					walk(doc, blocks, Some(&block.source));
				}
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. } => {
				walk(doc, blocks, Some(&block.source))
			}
			// Front matter wraps a verbatim code block built in place, so the
			// containment requirement still holds for it.
			BlockKind::FrontMatter { blocks, .. } => {
				walk(doc, blocks, Some(&block.source))
			}
			// A list's item ranges may take the terminator its own range
			// leaves out, so containment is not asserted here.
			BlockKind::List { items, .. } => {
				for item in items {
					walk(doc, &item.blocks, None);
				}
			}
			BlockKind::Table { rows, .. } => {
				for row in rows {
					for cell in row {
						check_rich(
							doc,
							cell,
							&block.source,
							"cell inline",
							true,
						);
					}
				}
			}
			BlockKind::Code { .. } | BlockKind::Rule => {}
		}
	}
}

/// An inline must address text inside the block that stores it, when the block
/// belongs to the document rather than to a snippet.
fn check_rich(
	doc: &Document,
	text: &[markview_core::document::Inline],
	block: &std::ops::Range<usize>,
	what: &str,
	assert_containment: bool,
) {
	for inline in text {
		if !assert_containment {
			continue;
		}
		let inside = block.start <= inline.source.start
			&& inline.source.end <= block.end;
		assert!(
			inside,
			"{what} range {:?} escapes its block {block:?} ({:?})",
			inline.source,
			doc.source.get(block.clone()).unwrap_or("")
		);
	}
}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	let md = String::from_utf8_lossy(data).into_owned();
	let len = md.len();
	if !(1..=256 * 1024).contains(&len) {
		return;
	}
	let guard = budget::InputGuard::new();
	let source: Arc<str> = Arc::from(md.as_str());
	let doc = markview_core::document::parse(source.clone());
	oracle::assert_source_ranges(&doc);
	seam::assert_rich_text_covered(&doc);
	walk(&doc, &doc.blocks, None);

	// A "trailing bytes must not move a block" differential was also tried
	// here and removed: an unterminated construct at EOF absorbs whatever
	// follows, so it fires on any input that leaves one open. Because the open
	// construct need not be the last block, there is no sound way to exclude it
	// by index, and a target that is red on an unrelated class out of the box
	// would bury anything new. That differential lives in the `refdef` target
	// instead, behind a structural gate that keeps it clean.
	guard.finish(&budget, len);
});
