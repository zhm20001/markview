use markview_core::document::{self, BlockKind};

#[test]
fn an_unused_reference_definition_keeps_preceding_details_unchanged() {
	for newline in ["\n", "\r\n", "\r"] {
		for body in [
			"+ \n  [^1]: x\n      ",
			"+ [used]\n      ",
			"\n```\n[used]",
			"[\n\n<?L:|LL\n  </other><?L:|LL\n ",
			"<details>+ \n  [^1]: x\n      </details>",
		] {
			for definitions in ["", "[used]: /used\n"] {
				let source = format!(
					"<details>{body}</details>\n\nEnd\n\n{definitions}"
				)
				.replace('\n', newline);
				let before = document::parse(source.clone());
				let after = document::parse(format!(
					"{source}{newline}[unused]: /unused{newline}"
				));
				assert_eq!(before.blocks, after.blocks, "{source:?}");
				if body.contains("```") || body.contains("<?") {
					let BlockKind::Details { blocks, .. } =
						&before.blocks[0].kind
					else {
						panic!("expected the details block")
					};
					let bare = document::parse(body.replace('\n', newline));
					let BlockKind::Code { language, text } =
						&blocks.last().unwrap().kind
					else {
						panic!("expected literal source")
					};
					let BlockKind::Code {
						language: expected_language,
						text: expected_text,
					} = &bare.blocks.last().unwrap().kind
					else {
						panic!("expected literal source")
					};
					assert_eq!(language, expected_language);
					assert_eq!(text, expected_text);
				}
			}
		}
	}
}

#[test]
fn an_unused_reference_definition_keeps_preceding_table_unchanged() {
	let source = "|---|:-:|---:|\n      |---|:-:|---:|\n|---|:-:|---:|\n|---|:-:|---|\n#\n";
	for newline in ["\n", "\r\n", "\r"] {
		let source = source.replace('\n', newline);
		let before = document::parse(source.clone());
		let after = document::parse(format!(
			"{source}{newline}[unused]: /unused{newline}"
		));
		assert_eq!(before.blocks, after.blocks);
		let BlockKind::Table { rows, .. } = &before.blocks[1].kind else {
			panic!("expected the table")
		};
		let cell = &rows[1][2][0];
		assert_eq!(&source[cell.source.clone()], "---");
	}
}
