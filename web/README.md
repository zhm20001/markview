# Markview Web demo

A browser page that renders Markdown onto a `<canvas>` through the Markview
engine compiled to WebAssembly: a textarea feeds Markdown, and `markview-core`
plus `markview-render` do the parsing, layout and painting. The frozen page
contract — JS API, page behavior and acceptance test — lives in
[docs/mvaac-web-demo.md](../docs/mvaac-web-demo.md).

`web/` is a **pnpm monorepo**: [`packages/markview`](packages/markview) is the
reusable `@markview/web` component, `apps/demo` is the thin private demo, and
`web/dist/` is the built, self-contained demo site that `serve.mjs` serves.

The demo consumes the package's **built** `dist/index.js`, not its sources, so
the build below also proves that a downstream bundler gets a working entry
point. The wasm binary travels as a plain sibling file named
`markview_web_bg.wasm`, which `init()` resolves against `import.meta.url`;
nothing depends on a bundler's asset handling. A deployment that serves the
JavaScript away from the binary passes `init({ wasmUrl })`.

## Build

Two steps, in order:

```sh
scripts/build-web.sh      # cargo build (wasm32) + wasm-bindgen -> web/packages/markview/wasm/
pnpm --dir web build      # esbuild + tsc: package dist and the demo site in web/dist/
```

`scripts/build-web.sh` needs `wasm-bindgen` 0.2.129 — vendored under `.tools/`
and picked up through `$WASM_BINDGEN`, or installed with `cargo install
wasm-bindgen-cli --version 0.2.129`. Everything under `web/dist/`,
`web/*/dist/` and `web/packages/markview/wasm/` is generated and gitignored.

## Serve

```sh
node web/serve.mjs    # or: pnpm --dir web serve; PORT selects the port, default 4173
```

Then open `http://127.0.0.1:4173/`. The server is dependency-free, serves the
built site in `web/dist/` with `Cache-Control: no-store`, and gives `.wasm`
files the `application/wasm` MIME type the WebAssembly fetch requires.

## Test

The Playwright acceptance suite drives engine startup, rendering,
selection, copy, incremental re-layout, the resumable layout API, the reader's
lifecycle, reflow on a narrower canvas and the scroll range against the built
demo in `web/dist/`:

```sh
pnpm --dir web test                      # headless, SwiftShader (works anywhere)
MV_GPU=1 pnpm --dir web test --project=chromium-gpu   # the real GPU
```

Headless Chromium picks SwiftShader unless ANGLE is pointed at the platform
driver, so the default project rasterizes on the CPU. The `chromium-gpu`
project adds `--use-angle=vulkan --enable-features=Vulkan` and is opt-in,
because a machine with no Vulkan driver should fail loudly rather than silently
fall back. Each run prints the device it used:

```
[device] wgpu adapter: ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) …), SwiftShader driver) (Gl, Cpu)
[device] wgpu adapter: ANGLE (Intel, Vulkan 1.4.354 (Intel(R) Arc(tm) Graphics (MTL) …), Intel open-source Mesa driver) (Gl, IntegratedGpu)
```

## `@markview/web`

The reusable component. Call `init()` once, then use the surface:

```js
import init, { Markview, CanvasReader } from "@markview/web";
await init({ fonts: ["/fonts/NotoSerif-Regular.otf", "/fonts/NotoSans-Regular.otf"] });

// Low level: full control of the handle.
const mv = await Markview.create(canvas, { fontSize: 19 });
mv.setMarkdown("# Hello");          // complete layout, published
const update = mv.beginLayout(md);  // resumable: see below
console.log(mv.stats());            // MarkviewStats (blocks, glyphs, pending, ...)

// High level: CanvasReader owns the rAF loop, sizing and input.
const reader = await CanvasReader.attach(canvas, {
  markdown,
  markview: { theme: "dark" },
  stepBudgetMs: 8,
  onStats: (s) => updateHud(s),
});
```

`Markview` is the handle (`setMarkdown`, selection, copy, `stats()`, …);
`CanvasReader` is the convenience that keeps the demo a wiring file. See
`packages/markview/dist/index.d.ts` and the contract for the full surface.

## Progressive layout

Long documents need not be laid out in one go. `beginLayout` starts a pass and
returns a `LayoutUpdate` handle; each `step()` lays out for at most a budget of
milliseconds and publishes the prefix it finished, so a caller can drive it
from `requestAnimationFrame` and render after every step:

```js
const update = mv.beginLayout(longMarkdown);
while (!update.step(8)) {
  mv.frame();            // present the prefix laid out so far
}
```

`step()` never re-lays-out what an earlier call already laid out — the cost of
a step does not grow with the already-published prefix. `finish()` completes
the same pass synchronously, and `update.blocks` / `update.done` report
progress.

## Injecting configuration

Set `window.MV_CONFIG` to an object or a JSON string before the module runs;
the demo hands it to `Markview.create()`, and the engine fills in every key the
config leaves out. Recognised keys: `width`, `fontSize`, `theme`, `justify`,
`hyphenate`, `paragraphIndent`, `greedy`, `hideFrontMatter`,
`frontMatterLabel`:

```js
window.MV_CONFIG = { fontSize: 20, theme: "dark" };
```

## Known limitations

- No tabs, settings UI or font management: the demo is a single document.
- Configuration is injected from JavaScript only — there is no file IO.
- Only the pinned subset faces (host assets imported by `apps/demo/src/fonts.ts`)
  ship with the demo, so exotic scripts may render as tofu. Check any
  document's coverage with `python3 scripts/check_web_font_coverage.py [document]`.
  That check unions every bundled face, so a character only one face carries is
  still reported as covered when the family a body run reaches cannot fall back
  to it: keep an eye on the canvas for a style gap as well.

## Interaction testing

The demo's **Scroll** selector switches between `internal` easing and
`external` direct motion without rebuilding. **Interaction sample** loads
links, a hidden heading in details, a wide code block and a long document for
wheel and selection testing. `internal` is the default; hosts can explicitly choose `external` for
input whose motion is already maintained outside Markview. Link and image callbacks display their targets in the
demo's notice area; the component does not navigate external links by itself.

Hosts can pass `scrollMode`, `onLink` and `onImage` to `CanvasReader.attach`.
Direct integrations can use `scrollInput`, `cursor`, `pointerLeave`,
`cancelPointer` and the activation returned by `pointerUp`; see the contract
for the types and motion ownership semantics.

## Host fonts

The wasm binary embeds only KaTeX's math fonts. Hosts supply text fonts through
`init({ fonts })` before creating any `Markview` or `CanvasReader`:

```ts
import init, { type FontSource } from "@markview/web";

const fonts: FontSource[] = [
  "/fonts/NotoSerif-Regular.otf",
  new URL("./fonts/NotoSerif-Bold.otf", import.meta.url),
  fontArrayBuffer,
  fontUint8Array,
];
await init({ fonts });
```

URL sources are fetched in parallel with wasm initialization. Byte sources
are copied into wasm, preserving a `Uint8Array` view's offset and length.
OpenType (`.otf`), TrueType (`.ttf`) and font collections (`.ttc`/`.otc`) are
supported; WOFF/WOFF2 and CSS `@font-face` fonts are not used by the engine.
HTTP failures and invalid font files reject initialization; callers can retry
with corrected sources. Concurrent and later `init()` calls share the first
successful initialization's options and fonts, so every reader reuses the
same collection. Omitting `fonts` makes no text fonts available.
Supply at least one text face for paragraph metrics, including math placement.

Supply regular, bold, italic and script fallback faces as needed. Family
names are read from the files; the bundled stylesheet looks for Noto Serif,
Noto Sans, Noto Sans Mono and their CJK variants, among other families.

The demo imports its 16 pinned subset faces in `apps/demo/src/fonts.ts`.
esbuild's `file` loaders emit them as `assets/[name]-[hash].otf`/`.ttf`, and
the demo hands those URLs to `init`. The reusable package ships no text fonts.
Deploy the generated `assets/` directory along with the demo's JavaScript
and wasm binary.

## Host-managed asynchronous images

Image loading is opt-in and separate from typography options. Supply
`resources` to `CanvasReader.attach`, or the third argument to `Markview.create`:

```ts
import { CanvasReader, loadImageUrl } from "@markview/web";

const reader = await CanvasReader.attach(canvas, {
  markdown: "![Example](images/example.png)",
  resources: {
    onResources(events) {
      for (const event of events) {
        if (event.kind === "request") void loadImageUrl(event.request);
      }
    },
  },
});
```

The callback receives all deduplicated image requests in a microtask, including
images outside the viewport and in closed details. A request exposes `src`, a
unique `id`, `signal`, current `priority`, and `resolve(pixels)` / `reject(message)`.
A custom host can queue requests, fetch authenticated bytes, then call
`request.resolve(await decodeImage(bytes, request.signal))`. Catch asynchronous
failures and call `reject`; the callback's return value is ignored.

`priority` events report changes among `visible`, `near`, `offscreen`, and
`unknown`, with vertical distance in CSS pixels. The host owns concurrency,
throttling and caching. No resources are fetched when no callback is supplied.
The URL helper accepts `{ baseUrl, requestInit }`, obeys browser CORS, and
supports HTTP(S), Blob and `data:image/` URLs. Browser decoding displays one
static frame and determines format support.
`decodeImage` infers the SVG MIME type for bytes and untyped Blobs, including
typed-array views that contain only part of a larger buffer.

Pixels use `{ width, height, rgba: Uint8Array }` in straight-alpha sRGB RGBA8.
They are copied on completion and published in batches through progressive
reflow. Low-level hosts drive `stepPending()` and `frame()` after completion;
`CanvasReader` already does this. An image reflow supersedes a `LayoutUpdate`,
so that handle becomes stale. Layout completion does not wait for images, and
`stats.pending` remains a layout counter.

Resize and option changes retain requests. Replacing Markdown or destroying
the component aborts their signals; late and duplicate results are ignored.
Failures remain error placeholders until the document is replaced. Recoverable
callback errors use `resources.onError`, then the reader's `onError`, or the
console. Text fonts still load through `init({ fonts })`; Mermaid integration
is separate from external resource loading.
