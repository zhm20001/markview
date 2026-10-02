// Image request events are independent of layout options and host scheduling.

/** Straight-alpha sRGB RGBA8, copied when the request is resolved. */
export interface ImagePixels {
	width: number;
	height: number;
	rgba: Uint8Array;
}

export interface ImagePriority {
	region: "visible" | "near" | "offscreen" | "unknown";
	/** Shortest vertical distance to the viewport, in CSS px. */
	distance: number | null;
}

export interface ImageRequest {
	readonly id: string;
	readonly src: string;
	readonly signal: AbortSignal;
	/** Current priority; changes are also delivered as events. */
	readonly priority: ImagePriority;
	resolve(pixels: ImagePixels): void;
	reject(message: string): void;
}

export type ImageResourceEvent =
	| { kind: "request"; request: ImageRequest }
	| { kind: "priority"; request: ImageRequest };

export interface ResourceOptions {
	/** Receives all requests; the host owns concurrency, caching and throttling. */
	onResources?: (events: readonly ImageResourceEvent[]) => void;
	/** Recoverable callback errors; defaults to `console.error`. */
	onError?: (error: unknown) => void;
}

interface ResourceHandle {
	imageSources(): string;
	imagePriorities(): string;
	resolveImage(generation: string, src: string, width: number, height: number, rgba: Uint8Array): void;
	rejectImage(generation: string, src: string, message: string): void;
	flushImages(): boolean;
}

interface Entry {
	request: ImageRequest;
	controller: AbortController;
	priority: ImagePriority;
	settled: boolean;
	delivered: boolean;
}
type Completion = { src: string; pixels: ImagePixels } | { src: string; error: string };
let nextOwner = 0;

/** @internal Transport bookkeeping; never fetches or schedules host work. */
export class ResourceEvents {
	readonly #owner = ++nextOwner;
	readonly #options: ResourceOptions;
	#generation = "";
	#entries = new Map<string, Entry>();
	#completions: Completion[] = [];

	constructor(options: ResourceOptions = {}) {
		this.#options = options;
	}

	replace(handle: ResourceHandle): void {
		this.clear();
		const { generation, sources } = JSON.parse(handle.imageSources()) as { generation: string; sources: string[] };
		this.#generation = generation;
		if (!this.#options.onResources) return;
		const events: ImageResourceEvent[] = [];
		for (const [index, src] of sources.entries()) {
			const controller = new AbortController();
			const current = (): boolean => !entry.settled && !controller.signal.aborted && this.#generation === generation;
			const request: ImageRequest = {
				id: `${this.#owner}:${generation}:${index}`, src, signal: controller.signal,
				get priority() { return entry.priority; },
				resolve: (pixels) => {
					if (!current()) return;
					const { width, height, rgba } = pixels;
					if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0
						|| width > 0xffffffff || height > 0xffffffff || !(rgba instanceof Uint8Array)
						|| width * height * 4 !== rgba.byteLength) {
						request.reject("Invalid image dimensions or RGBA length");
						return;
					}
					entry.settled = true;
					this.#completions.push({ src, pixels: { width, height, rgba: rgba.slice() } });
				},
				reject: (message) => {
					if (!current()) return;
					entry.settled = true;
					this.#completions.push({ src, error: message });
				},
			};
			const entry: Entry = { request, controller, priority: { region: "unknown", distance: null }, settled: false, delivered: false };
			this.#entries.set(src, entry);
			events.push({ kind: "request", request });
		}
		queueMicrotask(() => {
			if (this.#generation === generation) {
				for (const entry of this.#entries.values()) entry.delivered = true;
				this.#emit(events);
			}
		});
	}

	flush(handle: ResourceHandle): boolean {
		const completions = this.#completions;
		this.#completions = [];
		for (const result of completions) {
			if ("pixels" in result) {
				const { width, height, rgba } = result.pixels;
				handle.resolveImage(this.#generation, result.src, width, height, rgba);
			} else handle.rejectImage(this.#generation, result.src, result.error);
		}
		return handle.flushImages();
	}

	priorities(handle: ResourceHandle): void {
		if (![...this.#entries.values()].some((entry) => entry.delivered && !entry.settled)) return;
		const priorities = JSON.parse(handle.imagePriorities()) as Record<string, ImagePriority>;
		const events: ImageResourceEvent[] = [];
		for (const [src, entry] of this.#entries) {
			if (entry.settled || !entry.delivered) continue;
			const next = priorities[src] ?? { region: "unknown", distance: null };
			if (next.region !== entry.priority.region || next.distance !== entry.priority.distance) {
				entry.priority = next;
				events.push({ kind: "priority", request: entry.request });
			}
		}
		this.#emit(events);
	}

	clear(): void {
		this.#generation = "";
		this.#completions = [];
		const entries = this.#entries;
		this.#entries = new Map();
		for (const entry of entries.values()) entry.controller.abort();
	}

	#emit(events: readonly ImageResourceEvent[]): void {
		if (!events.length) return;
		try {
			this.#options.onResources?.(events);
		} catch (error) {
			for (const event of events) {
				if (event.kind === "request") event.request.reject(String(error));
			}
			try {
				if (this.#options.onError) this.#options.onError(error);
				else console.error("markview resources:", error);
			} catch (reportError) { console.error("markview resources:", reportError); }
		}
	}
}
