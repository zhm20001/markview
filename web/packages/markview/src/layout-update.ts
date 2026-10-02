// A resumable layout of one document: `beginLayout()` hands one of these out,
// the caller drives `step()` from a frame loop and the engine publishes each
// finished prefix immediately.

import { parseStats } from "./internal.js";

/**
 * The slice of the wasm handle a layout update drives. The generated glue type
 * is structurally assignable to it, and declaring it here keeps the emitted
 * declarations free of an import of generated glue the package does not ship.
 */
interface PendingHandle {
	updatePending(): boolean;
	stepUpdate(budgetMs: number): string;
	finishUpdate(): void;
	stats(): string;
}

/** A layout in progress; every block is laid out at most once. */
export class LayoutUpdate {
	/**
	 * A live wasm handle, asked for on every call rather than kept: a handle
	 * the owning `Markview.destroy()` released then raises that class's own
	 * error instead of a null-pointer one.
	 */
	readonly #live: () => PendingHandle;

	/** Whether this update still owns the handle's pending pass. */
	readonly #current: () => boolean;

	/** The revision published when this pass began. */
	readonly #baseline: number;
	readonly #prepare: () => void;

	/**
	 * The blocks this layout has published. It starts at zero and follows the
	 * revision: a step that did not advance it published nothing, so the
	 * blocks in `stats` still belong to the document this pass replaces.
	 */
	#published = 0;

	/** @internal Created only by `Markview.beginLayout`. */
	constructor(live: () => PendingHandle, current: () => boolean, baseline: number, prepare: () => void) {
		this.#live = live;
		this.#current = current;
		this.#baseline = baseline;
		this.#prepare = prepare;
	}

	/**
	 * Whether a later `beginLayout` replaced this pass. A superseded update is
	 * inert: it reports `done` and neither steps nor finishes, so it can never
	 * drive the document that replaced it.
	 */
	get stale(): boolean {
		return !this.#current();
	}

	/** Blocks this layout has published so far. */
	get blocks(): number {
		this.#live();
		return this.#published;
	}

	/** Whether the whole document is laid out. */
	get done(): boolean {
		return this.stale || !this.#live().updatePending();
	}

	/** Lays out for at most `budgetMs` (default 8). Returns `done`. */
	step(budgetMs?: number): boolean {
		if (this.stale) return true;
		this.#prepare();
		if (this.stale) return true;
		const budget = budgetMs ?? 8;
		const stats = parseStats(
			this.#live().stepUpdate(Number.isFinite(budget) ? budget : 8),
		);
		// A replacement pass keeps the previous snapshot on screen until its own
		// prefix reaches the scroll offset, so an unadvanced revision means the
		// blocks belong to the document this pass replaced, not to this one.
		if (stats.revision !== this.#baseline) {
			this.#published = stats.blocks;
		}
		return this.done;
	}

	/** Completes the layout synchronously. */
	finish(): void {
		if (this.stale) return;
		this.#prepare();
		if (this.stale) return;
		const handle = this.#live();
		handle.finishUpdate();
		this.#published = parseStats(handle.stats()).blocks;
	}
}
