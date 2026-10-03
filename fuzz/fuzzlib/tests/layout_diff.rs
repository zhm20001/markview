use std::sync::{Arc, Mutex};

use markview_core::{
	background::{Executor, Task},
	document,
	layout::LayoutEngine,
	scene::Draw,
};
use mvfuzz::{oracle, pipeline};

#[derive(Default)]
struct Deferred(Mutex<Vec<Task>>);

impl Executor for Deferred {
	fn try_submit(&self, task: Task) -> Result<(), Task> {
		self.0.lock().unwrap().push(task);
		Ok(())
	}
}

#[test]
fn code_highlights_change_only_paint_and_differentials_start_settled() {
	for code in ["~\0~", "\0", "let value = 42;"] {
		let md = format!("```rust\n{code}\n```\n");
		let doc = document::parse(md.clone());
		let options = pipeline::options_for(&md);
		let executor = Arc::new(Deferred::default());
		let mut delayed =
			LayoutEngine::with_executor(executor.clone(), Arc::new(|| {}));
		let mut cold = delayed.layout(&doc, &options);
		for task in executor.0.lock().unwrap().drain(..) {
			task.run();
		}
		assert!(delayed.wait_highlights());
		let settled = delayed.layout(&doc, &options);
		assert_ne!(oracle::layout(&cold), oracle::layout(&settled));
		delayed.release_document();

		// Copy only paints; every other fingerprinted field must already match.
		for (a, b) in cold.blocks.iter_mut().zip(&settled.blocks) {
			for (a, b) in Arc::get_mut(&mut a.layout)
				.unwrap()
				.draws
				.iter_mut()
				.zip(&b.layout.draws)
			{
				if let (Draw::Glyph(a), Draw::Glyph(b)) = (a, b) {
					a.paint = b.paint;
				}
			}
		}
		assert_eq!(oracle::layout(&cold), oracle::layout(&settled));

		let mut engine = pipeline::differential_engine();
		let first = engine.layout(&doc, &options);
		let cached = engine.layout(&doc, &options);
		let fresh = pipeline::differential_engine().layout(&doc, &options);
		assert_eq!(oracle::layout(&first), oracle::layout(&settled));
		assert_eq!(oracle::layout(&first), oracle::layout(&cached));
		assert_eq!(oracle::layout(&cached), oracle::layout(&fresh));
		assert_eq!(cached.reused, cached.blocks.len());
	}
}
