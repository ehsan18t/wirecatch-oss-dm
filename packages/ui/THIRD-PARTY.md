# Third-party notices for @wirecatch/ui

The package as a whole is GPL-3.0-only, like the rest of Wirecatch. Parts of it come from the works below, whose licences allow that and ask for their notices to travel with them.

## MagicX Toolbox

Source: the MagicX Toolbox app (`magicx-toolbox-gui`), by Ehsan Khan, MIT. Wirecatch's design system is adapted from its design tokens, motion presets and generic interface components, rewritten to Wirecatch's rules. These files in `src/` derive from it:

- `tokens.css` and `base.css`: the type scale and fonts, motion durations, delays, easings and keyframes, the z-index scale, the reduced-motion rule, the drag-region utilities and the `enabled` variant.
- `motion/motion.ts` and `motion/easing.ts`: the fade, shift, pop, expand, glide and reflow presets and the cubic-Bézier easing.
- `lib/cn.ts`, `lib/css-token.ts`, `lib/flyout.ts`, `lib/focus.ts`, `lib/list-nav.ts`, `lib/percent.ts`: class merging with the theme's names, token reading, flyout placement, focus helpers and list keys.
- `attachments/tooltip.svelte.ts`: the tooltip attachment and `textIfCut`.
- `recipes/` and `internal/`: the button, field, card and tone recipes, and the disabled-with-a-reason pattern.
- `primitives/`: Button, IconButton, Icon, Badge, Callout, Card, Checkbox, EmptyState, ProgressBar, Select, SegmentedControl, Skeleton, Spinner, Switch and TextArea.
- `shell/`: Dialog (from Modal), the sidebar's rail and drawer modes, the caption buttons and title bar layout, and the toast queue and region.

```
MIT License

Copyright (c) 2025 Ehsan Khan

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## Lucide icons

The icons come from Lucide (https://lucide.dev), through Iconify's `@iconify-json/lucide` set, which unplugin-icons compiles into the apps at build time. Lucide is ISC; the portions that came from Feather are MIT.

```
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

Feather's licence, which travels with the portions of Lucide that came from it:

```
The MIT License (MIT)

Copyright (c) 2013-2023 Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```
