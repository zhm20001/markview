use markview_core::scene::{
	BlockLayout, Draw, LayoutSnapshot, LinkRect, Overflow, PlacedBlock, Rect,
	ScrollbarMetrics, Viewport,
};
use markview_selection::{
	Cursor, DocumentInteraction, Horizontal, Motion, Point, ScrollBounds,
	ScrollState, horizontal_by, selection_scroll,
};
use std::{sync::Arc, time::Duration};
use web_time::Instant;

fn snapshot() -> LayoutSnapshot {
	let rect = Rect {
		x: 0.0,
		y: 0.0,
		w: 100.0,
		h: 30.0,
	};
	LayoutSnapshot {
		height: 100.0,
		width: 100.0,
		blocks: vec![PlacedBlock {
			id: 1,
			source: 0..0,
			y: 0.0,
			layout: Arc::new(BlockLayout {
				height: 100.0,
				draws: vec![Draw::Image {
					rect,
					src: "image".into(),
					version: 1,
					title: "Image title".into(),
				}],
				links: vec![LinkRect {
					rect: Rect { w: 30.0, ..rect },
					url: "https://example.com".into(),
					command: 0,
				}],
				overflow: vec![Overflow {
					rect,
					content_width: 300.0,
					gutter: 12.0,
					commands: 0..1,
				}],
				..Default::default()
			}),
		}],
		..Default::default()
	}
}

#[test]
fn hover_and_hits_follow_viewport_and_horizontal_motion() {
	let snapshot = snapshot();
	let mut horizontal = Horizontal::default();
	let viewport = Viewport {
		width: 200.0,
		height: 100.0,
		left: 20.0,
		top: 10.0,
		bottom: 10.0,
		scroll: 0.0,
	};
	let point = Point::new(25.0, 15.0);
	let metrics = ScrollbarMetrics::default();
	let context = DocumentInteraction {
		snapshot: &snapshot,
		viewport,
		horizontal: &horizontal,
		revision: 1,
	};
	assert_eq!(
		context.hover(point, false, false, metrics).cursor,
		Cursor::Pointer
	);
	assert!(context.hover(point, true, true, metrics).link.is_none());
	let image = Point::new(80.0, 15.0);
	assert_eq!(
		context.hover(image, false, false, metrics).cursor,
		Cursor::Default
	);
	assert_eq!(
		context.hover(image, false, true, metrics).cursor,
		Cursor::Pointer
	);
	assert_eq!(
		context
			.hover(image, false, true, metrics)
			.image_title
			.as_deref(),
		Some("Image title")
	);
	let bar = context.overflow_bar(0, 0, metrics).unwrap();
	let hit = Point::new(
		bar.track.x + bar.track.w * 0.5,
		bar.track.y + bar.track.h * 0.5,
	);
	assert!(context.overflow_bar_at(hit, metrics).is_some());
	assert_eq!(
		context.hover(hit, false, false, metrics).overflow,
		Some((0, 0))
	);
	assert!(context.link(Point::new(25.0, 5.0)).is_none());
	assert!(horizontal_by(
		&snapshot,
		viewport,
		&mut horizontal,
		point,
		500.0
	));
	assert_eq!(horizontal[&(0, 0)], 200.0);
	let context = DocumentInteraction {
		snapshot: &snapshot,
		viewport,
		horizontal: &horizontal,
		revision: 1,
	};
	assert!(context.link(point).is_none());
	let context = DocumentInteraction {
		viewport: Viewport {
			scroll: 50.0,
			..viewport
		},
		..context
	};
	assert!(context.link(point).is_none());
}

#[test]
fn external_input_takes_over_and_pending_targets_survive_layout() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 1000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	scroll.wheel_by(300.0, start);
	scroll.wheel_by(300.0, start);
	assert_eq!(scroll.target, Some(600.0));
	scroll.advance(start + Duration::from_millis(40), bounds);
	let visible = scroll.offset;
	scroll.wheel_by(-20.0, start + Duration::from_millis(40));
	assert_eq!(scroll.target, Some(visible - 20.0));
	scroll.by(10.0, bounds);
	assert_eq!(scroll.offset, visible + 10.0);
	assert!(scroll.animation.is_none());
	let prefix = ScrollBounds {
		max: 100.0,
		complete: false,
	};
	scroll.set(900.0, prefix);
	assert_eq!(scroll.visible(prefix), 100.0);
	let growing = ScrollBounds {
		max: 500.0,
		complete: false,
	};
	scroll.resolve(growing);
	assert_eq!(scroll.offset, 900.0);
	assert_eq!(scroll.visible(growing), 500.0);
	assert_eq!(scroll.target, Some(900.0));
	scroll.resolve(bounds);
	assert_eq!(scroll.offset, 900.0);
	scroll.target = Some(f32::INFINITY);
	scroll.by(0.0, prefix);
	assert_eq!(scroll.target, Some(f32::INFINITY));
	scroll.resolve(bounds);
	assert_eq!(scroll.offset, 1000.0);
}

#[test]
fn gesture_inertia_and_selection_edges_stop() {
	let start = Instant::now();
	let mut motion = Motion::new(start);
	motion.sample((0.0, 30.0), start + Duration::from_millis(20));
	assert!(motion.release(start + Duration::from_millis(25)));
	assert!(motion.advance(start + Duration::from_millis(40)).unwrap().1 > 0.0);
	assert!(motion.advance(start + Duration::from_secs(1)).is_none());
	assert_eq!(selection_scroll(5.0, 10.0, 100.0, 0.0, 500.0), 0.0);
	assert_eq!(selection_scroll(5.0, 10.0, 100.0, 50.0, 500.0), -14.0);
	assert_eq!(selection_scroll(110.0, 10.0, 100.0, 50.0, 500.0), 14.0);
	assert_eq!(selection_scroll(110.0, 10.0, 100.0, 500.0, 500.0), 0.0);
}

#[test]
fn wheel_packet_stream_rides_across_gaps_and_settles() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// A fast two-finger scroll: packets 40 ms apart, with the frame loop
	// running between them as the event loop would.
	let mut at = start;
	for _ in 0..5 {
		at += Duration::from_millis(40);
		scroll.coast_wheel_by(120.0, at);
		for step in 1..=4 {
			scroll.advance(at + Duration::from_millis(step * 8), bounds);
		}
	}
	// The stream keeps a speed of its own, so the page has ridden well past
	// where easing each packet from a standstill would have left it.
	assert!(
		scroll.offset > 300.0,
		"the page should ride the stream, at {}",
		scroll.offset
	);
	assert_eq!(scroll.target, Some(600.0));
	// Frames keep coming while the momentum lasts. When the speed dies the
	// stream is spent and the page rests at the lead it earned.
	let mut frame = at;
	for _ in 0..400 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
	}
	assert!(!scroll.animating());
	assert!(scroll.offset > 600.0 && scroll.offset < 1200.0);
	assert_eq!(scroll.target, None);
}

#[test]
fn wheel_packet_after_a_pause_answers_a_reversal() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// A downward flick hands its inertia over in packets, and the page
	// rides on after the last one.
	let mut at = start;
	for _ in 0..4 {
		at += Duration::from_millis(40);
		scroll.coast_wheel_by(120.0, at);
		for step in 1..=4 {
			scroll.advance(at + Duration::from_millis(step * 8), bounds);
		}
	}
	let ridden = scroll.offset;
	// After the stream has gone quiet the hand reverses. The speed the old
	// gesture left behind is spent, so the page answers the upward packet
	// instead of carrying on downward past it.
	at += Duration::from_millis(200);
	scroll.coast_wheel_by(-300.0, at);
	let destination = (ridden - 300.0).max(0.0);
	assert_eq!(scroll.target, Some(destination));
	let mut frame = at;
	for _ in 0..400 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
	}
	assert!(
		scroll.offset < ridden,
		"the page should have turned, at {ridden}"
	);
	assert!((scroll.offset - destination).abs() < 1.0);
	assert_eq!(scroll.target, None);
}

#[test]
fn direct_and_eased_steps_take_over_from_a_wheel_stream() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	for eased in [false, true] {
		let mut scroll = ScrollState::default();
		scroll.coast_wheel_by(120.0, start);
		scroll.coast_wheel_by(120.0, start + Duration::from_millis(40));
		scroll.advance(start + Duration::from_millis(48), bounds);
		let displayed = scroll.offset;
		assert!(scroll.animating());
		if eased {
			scroll.animate_by(10.0, start + Duration::from_millis(48));
			scroll.advance(start + Duration::from_secs(1), bounds);
		} else {
			scroll.by(10.0, bounds);
		}
		assert_eq!(scroll.offset, displayed + 10.0);
		assert!(!scroll.animating());
		assert_eq!(scroll.target, None);
	}
}

#[test]
fn cancelling_a_wheel_stream_forgets_its_target_when_layout_grows() {
	let start = Instant::now();
	let prefix = ScrollBounds {
		max: 100.0,
		complete: false,
	};
	let mut scroll = ScrollState::default();
	scroll.coast_wheel_by(120.0, start);
	scroll.coast_wheel_by(120.0, start + Duration::from_millis(40));
	scroll.advance(start + Duration::from_millis(48), prefix);
	let displayed = scroll.offset;
	scroll.cancel();
	assert!(!scroll.animating());
	assert_eq!(scroll.target, None);
	assert_eq!(scroll.deadline(start + Duration::from_millis(48)), None);
	scroll.resolve(ScrollBounds {
		max: 4000.0,
		complete: true,
	});
	assert_eq!(scroll.offset, displayed);
	// A direct request beyond an incomplete prefix still survives cancellation.
	scroll.set(900.0, prefix);
	scroll.cancel();
	assert_eq!(scroll.target, Some(900.0));
}

#[test]
fn a_wheel_packet_after_a_pause_follows_the_new_input() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	for delta in [-10.0, 10.0] {
		let mut scroll = ScrollState::default();
		for frame in 0..=45 {
			let at = start + Duration::from_millis(frame * 8);
			if frame <= 15 && frame % 5 == 0 {
				scroll.coast_wheel_by(120.0, at);
			}
			scroll.advance(at, bounds);
		}
		let displayed = scroll.offset;
		assert!(displayed > 480.0 && scroll.animating());
		scroll.coast_wheel_by(delta, start + Duration::from_millis(368));
		assert_eq!(scroll.target, Some(displayed + delta));
		for frame in 47..=300 {
			let before = scroll.offset;
			scroll.advance(start + Duration::from_millis(frame * 8), bounds);
			assert!((scroll.offset - before) * delta >= 0.0);
		}
		assert!((scroll.offset - displayed - delta).abs() < 0.5);
		assert!(!scroll.animating());
	}
}

#[test]
fn a_small_wheel_reversal_discards_unpaid_travel() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	for direction in [-1.0, 1.0] {
		// Reverse both before a stream has velocity and while it is moving.
		for packets in [1, 2] {
			let mut scroll = ScrollState::default();
			scroll.set(1000.0, bounds);
			for packet in 0..packets {
				let at = start + Duration::from_millis(packet * 40);
				scroll.coast_wheel_by(120.0 * direction, at);
				for frame in 1..=5 {
					scroll
						.advance(at + Duration::from_millis(frame * 8), bounds);
				}
			}
			let displayed = scroll.offset;
			let at = start + Duration::from_millis(packets * 40);
			let delta = -10.0 * direction;
			scroll.coast_wheel_by(delta, at);
			assert_eq!(scroll.target, Some(displayed + delta));
			scroll.advance(at + Duration::from_millis(8), bounds);
			assert!((scroll.offset - displayed) * delta > 0.0);
		}
	}
}
