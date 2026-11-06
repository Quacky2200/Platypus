// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

/**
 * Class names joined by spaces, empty ones left out: `cx("row", selected ? "selected" : "")`.
 * In place of `["row", selected && "selected"].filter(Boolean).join(" ")`: web_ui_ts makes
 * an array of mixed types a tuple (tools/web_ui_ts/TODO.md, "Arrays of mixed types").
 */
export function cx(...names: string[]): string {
  return names.filter((n) => n !== "").join(" ");
}
