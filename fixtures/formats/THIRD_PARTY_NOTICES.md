# Third-party format fixtures

## libheif example images

The following unmodified files are redistributed from the official
[`strukturag/libheif`](https://github.com/strukturag/libheif) repository:

- `sources/avif/libheif-example.avif`, pinned to commit
  `3a6997e8c4d4df7c20dfcb2937484630e05f5570`;
- `sources/heic/libheif-example.heic`, pinned to commit
  `b97d0c2b2353c8c132f334729fc75e2d47d3763d`.

The upstream `examples/COPYING` file applies the following license:

> MIT License
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

The pinned upstream license is available at
[`examples/COPYING`](https://raw.githubusercontent.com/strukturag/libheif/b97d0c2b2353c8c132f334729fc75e2d47d3763d/examples/COPYING).
`tools/import-format-fixtures.mjs` reimports only the pinned bytes and refuses a
size or SHA-256 mismatch.
