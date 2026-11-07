# Markit Showcase

A tour of the **Markdown** features Markit renders *live* while you type.

## Type styles

**Strong**, *emphasis*, ***both***, ~~strikethrough~~, `inline code`, and a [hyperlink](https://tauri.app).

> Blockquotes hold quoted text.
> They can span several lines.

### Lists

- Unordered item
- Another item
  - Nested item

1. First
2. Second

- [x] Finished task
- [ ] Open task

## Table

| Feature | Status | Notes |
|:--------|:------:|------:|
| Tables | ✅ | add rows & columns |
| Themes | ✅ | four built in |
| PDF | ✅ | paginated |

## Code

```rust
fn main() {
    println!("Hello from Markit");
}
```

## Math

Inline math: $E = mc^2$.

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$

## Image

![Markit logo](assets/markit-logo.png)

---

Footnotes work in export.[^1]

[^1]: Rendered by comrak.
