//! Budget calibration (B2, B3).
//!
//! Measures per-input wall time and peak live allocation over a
//! representative input set for the parse, layout, and PDF export paths,
//! then reports the high percentile, the maximum, and a linear fit of
//! peak allocation against input length. The recommended budget is the
//! maximum times a safety factor, rounded up; the record lists the inputs,
//! this machine, and the date so the numbers stay auditable.
//!
//! Run: `cargo run -p mvfuzz --bin calibrate -- --out <path>`

use std::{
	env, fs,
	path::{Path, PathBuf},
	sync::Arc,
	time::Instant,
};

use mvfuzz::allocator;
use mvfuzz::pipeline::{export_pdf, parse_layout};

#[derive(Default)]
struct Sample {
	time_ms: u128,
	alloc: u64,
}

fn run(parse: fn(&str) -> Sample, input: &str) -> Sample {
	let base = allocator::GLOBAL.peak();
	let start = Instant::now();
	parse(input);
	Sample {
		time_ms: start.elapsed().as_millis(),
		alloc: allocator::GLOBAL.peak().saturating_sub(base),
	}
}

fn parse_sample(input: &str) -> Sample {
	run(
		|s| {
			// Both parse shapes: the full parse and the incremental reparse of
			// the unchanged document, which the reader uses on every edit.
			let doc = markview_core::document::parse(s.to_string());
			let _ = markview_core::document::reparse(&doc, Arc::from(s));
			Sample::default()
		},
		input,
	)
}
fn layout_sample(input: &str) -> Sample {
	run(
		|s| {
			let _ = parse_layout(s);
			Sample::default()
		},
		input,
	)
}
fn pdf_sample(input: &str) -> Sample {
	// The export path needs real pixels for embedded images — inline,
	// through an `<img>` tag, or through a mermaid fence — so inputs that
	// embed one are measured with the layout step the export is built on.
	if input.contains("![")
		|| input.contains("<img")
		|| input.contains("mermaid")
	{
		return layout_sample(input);
	}
	run(
		|s| {
			let _ = export_pdf(s);
			Sample::default()
		},
		input,
	)
}

fn representative_inputs() -> Vec<String> {
	let mut out = Vec::new();
	// A few built-in extremes: long prose, a wide table, dense math, CJK.
	out.push("# Start\n\n".to_string()
		+ &"A paragraph of ordinary prose that wraps across the reading measure. ".repeat(2000)
			+ "\n");
	let table = (0..200)
		.map(|i| format!("| c{i} | value {i} | note {i} |"))
		.collect::<Vec<_>>()
		.join("\n");
	out.push(format!("| c | v | n |\n|---|---|---|\n{table}\n"));
	out.push(String::from("\n") + &"$\\frac{a}{b} + x^2$ ".repeat(300) + "\n");
	out.push(
		"中文段落，混合 English 与 **emphasis** 和 $x$。".repeat(500) + "\n",
	);
	out.push("```\n".to_string() + &"let x = 1;\n".repeat(2000) + "```\n");
	// Repository fixtures, when present; image-only fixtures export as empty
	// pages, so the ones that embed images are skipped for the PDF budget.
	let fixtures =
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
	if let Ok(entries) = fs::read_dir(&fixtures) {
		for entry in entries.flatten() {
			let path = entry.path();
			if path.extension().is_some_and(|e| e == "md")
				&& let Ok(text) = fs::read_to_string(&path)
			{
				out.push(text);
			}
		}
	}
	// The seed corpus, when the prepare script has run.
	let seeds =
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/parse");
	if let Ok(entries) = fs::read_dir(&seeds) {
		for entry in entries.flatten() {
			if let Ok(text) = fs::read_to_string(entry.path()) {
				out.push(text);
			}
		}
	}
	out
}

fn percentile(
	samples: &[Sample],
	field: fn(&Sample) -> u128,
	pct: f64,
) -> u128 {
	let mut v: Vec<u128> = samples.iter().map(field).collect();
	v.sort_unstable();
	let idx = ((v.len() as f64) * pct).ceil() as usize;
	v[idx.min(v.len() - 1)]
}

/// Least-squares fit `alloc ~= a * bytes + b` over the samples.
fn linear_fit(samples: &[(u64, u64)]) -> (u64, u64) {
	let n = samples.len() as f64;
	if n < 2.0 {
		return (samples.first().map(|(_, a)| *a).unwrap_or(0), 0);
	}
	let sx: f64 = samples.iter().map(|(x, _)| *x as f64).sum();
	let sy: f64 = samples.iter().map(|(_, y)| *y as f64).sum();
	let sxx: f64 = samples.iter().map(|(x, _)| (*x as f64) * (*x as f64)).sum();
	let sxy: f64 = samples.iter().map(|(x, y)| (*x as f64) * (*y as f64)).sum();
	let denom = n * sxx - sx * sx;
	let a = if denom.abs() < 1e-9 {
		0.0
	} else {
		(n * sxy - sx * sy) / denom
	};
	let b = (sy - a * sx) / n;
	(
		if a > 0.0 {
			(a * 1024.0).ceil() as u64
		} else {
			0
		},
		b.max(0.0) as u64,
	)
}

fn report(out: &Path) {
	let inputs = representative_inputs();
	// Each stage is capped by its target's own input limit, so a budget is
	// never set by an input the target refuses to run.
	type Stage<'a> = (&'a str, fn(&str) -> Sample, usize);
	let stages: [Stage; 3] = [
		("parse", parse_sample, 256 * 1024),
		("layout", layout_sample, 128 * 1024),
		("pdf", pdf_sample, 48 * 1024),
	];
	let mut md = String::new();
	md.push_str("# Budget calibration record\n\n");
	md.push_str(&format!(
		"- Date: {}\n- Machine: {}\n- Build: markview-fuzz @ {}\n- Inputs: {} \
			 (built-in extremes, `tests/fixtures`, and the parse seed corpus)\n\n",
		LocalDate::now(),
		std::env::consts::OS,
		env!("CARGO_PKG_VERSION"),
		inputs.len(),
	));
	for (name, f, cap) in stages {
		let samples: Vec<Sample> = inputs
			.iter()
			.filter(|i| i.len() <= cap)
			.map(|i| f(i))
			.collect();
		let times: Vec<u128> = samples.iter().map(|s| s.time_ms).collect();
		let p95 = percentile(&samples, |s| s.time_ms, 0.95);
		let max_t = times.iter().max().copied().unwrap_or(0);
		let max_a = samples.iter().map(|s| s.alloc).max().unwrap_or(0);
		let (per_kib, base) = linear_fit(
			&inputs
				.iter()
				.zip(&samples)
				.map(|(i, s)| (i.len() as u64, s.alloc))
				.collect::<Vec<_>>(),
		);
		md.push_str(&format!(
			"## {name}\n\n- wall ms: p95 {p95}, max {max_t}\n- peak alloc: max {max_a} \
			 bytes; linear fit base {base} + {per_kib} bytes/KiB\n- recommended: \
			 time {} ms, alloc base {} KiB, alloc {} KiB/KiB\n\n",
			(max_t * 4).max(1000),
			(max_a * 2) / 1024,
			(per_kib + 1024 * 1024) / 1024,
		));
	}
	md.push_str(
		"Method (B2): measure wall time and peak live allocation per input on the\n\
		 representative set; take the maximum times a safety factor (time x4, alloc x2)\n\
		 plus the linear fit's per-KiB term so the allowance scales with input\n\
		 length. Update `Budget::calibrated` in `fuzz/fuzzlib/src/budget.rs` when the\n\
		 numbers move meaningfully.\n",
	);
	if let Some(parent) = out.parent() {
		let _ = fs::create_dir_all(parent);
	}
	fs::write(out, md).unwrap();
	eprintln!("wrote {}", out.display());
}

struct LocalDate;
impl LocalDate {
	fn now() -> String {
		// Avoid a `chrono` dependency for one string.
		let secs = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map(|d| d.as_secs() as i64)
			.unwrap_or(0);
		let days = (secs / 86400).max(0);
		// Days from 1970-01-01 to a Y-M-D, civil algorithm.
		let z = days + 719_468;
		let era = z.div_euclid(146_097);
		let doe = z.rem_euclid(146_097);
		let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
		let y = yoe + era * 400;
		let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
		let mp = (5 * doy + 2) / 153;
		let d = doy - (153 * mp + 2) / 5 + 1;
		let m = if mp < 10 { mp + 3 } else { mp - 9 };
		let y = if m <= 2 { y + 1 } else { y };
		format!("{y:04}-{m:02}-{d:02}")
	}
}

fn main() {
	let out = env::args()
		.position(|a| a == "--out")
		.and_then(|i| env::args().nth(i + 1))
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from("budget-calibration.md"));
	report(&out);
}
