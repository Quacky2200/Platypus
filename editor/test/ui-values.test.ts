// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The UI inspector's value logic (tools/platypus_build/inspector/values.ts).

import {
  uiValColorFormat,
  uiValFindEasing,
  uiValFormatColor,
  uiValFormatEasing,
  uiValFormatGradient,
  uiValFormatNumber,
  uiValFromHsv,
  uiValGradientAngle,
  uiValNumbers,
  uiValParseColor,
  uiValParseGradient,
  uiValSplitTop,
  uiValStep,
  uiValStepSize,
  uiValSuggest,
  uiValToHsv,
} from "../../tools/platypus_build/inspector/values.ts";
import { close, eq, report, same, test, yes } from "./check.ts";

// M3-INS-11
test("numbers are found with their units and written without float noise", () => {
  const n = uiValNumbers("margin: 10px -4.5em 0 50%");
  eq(
    n.map((x) => [x.num, x.unit]),
    [[10, "px"], [-4.5, "em"], [0, ""], [50, "%"]],
    "numbers",
  );
  eq(uiValNumbers("#abc123").length, 0, "a hex colour holds no numbers");
  eq(uiValNumbers("translate3d(1px, 2px, 3px)").length, 3, "inside a call");
  eq(uiValNumbers("var(--x1)").length, 0, "a name with a digit");
  same(uiValFormatNumber(0.1 + 0.2), "0.3", "float noise");
  same(uiValFormatNumber(12), "12", "whole");
  same(uiValFormatNumber(-0.00001), "0", "negative zero");
});

// M3-INS-11
test("an arrow key moves the number at the caret by the step", () => {
  same(uiValStep("10px", 1, 1, -1)!, "11px", "first number, up");
  same(uiValStep("10px", -1, 1, -1)!, "9px", "down");
  same(uiValStep("1px 2px 3px", 1, 1, 6)!, "1px 3px 3px", "the number under the caret");
  same(uiValStep("1px 2px 3px", 1, 1, 4)!, "1px 3px 3px", "just after a number");
  same(uiValStep("0.5", 1, 0.1, -1)!, "0.6", "a fraction");
  same(uiValStep("calc(100% - 20px)", 1, 1, 14)!, "calc(100% - 21px)", "inside calc");
  same(uiValStep("1px 2px", 10, 1, -1)!, "11px 2px", "Shift is ten steps");
  eq(uiValStep("auto", 1, 1, -1), null, "no number");
  close(uiValStepSize("opacity", ""), 0.1, "opacity steps by a tenth");
  close(uiValStepSize("width", "px"), 1, "px steps by one");
  close(uiValStepSize("transitionDuration", "s"), 0.1, "seconds step by a tenth");
});

// M3-INS-11
test("colours are read in every CSS form and written in the one asked for", () => {
  eq(uiValParseColor("#f80"), { r: 255, g: 136, b: 0, a: 1 }, "short hex");
  eq(uiValParseColor("#11223344")!.a > 0.26 && uiValParseColor("#11223344")!.a < 0.28, true, "hex alpha");
  eq(uiValParseColor("rgb(10, 20, 30)"), { r: 10, g: 20, b: 30, a: 1 }, "rgb");
  eq(uiValParseColor("rgba(10 20 30 / 50%)"), { r: 10, g: 20, b: 30, a: 0.5 }, "rgb, space syntax");
  eq(uiValParseColor("hsl(120, 100%, 50%)"), { r: 0, g: 255, b: 0, a: 1 }, "hsl");
  eq(uiValParseColor("hsla(0, 100%, 50%, 0.25)"), { r: 255, g: 0, b: 0, a: 0.25 }, "hsla");
  eq(uiValParseColor("Orange"), { r: 255, g: 165, b: 0, a: 1 }, "a name");
  eq(uiValParseColor("transparent")!.a, 0, "transparent");
  eq(uiValParseColor("var(--x)"), null, "not a colour");
  eq(uiValParseColor("#12"), null, "bad hex");
  const c = { r: 255, g: 136, b: 0, a: 1 };
  same(uiValFormatColor(c, "hex"), "#ff8800", "hex");
  same(uiValFormatColor(c, "rgb"), "rgb(255, 136, 0)", "rgb");
  same(uiValFormatColor(c, "hsl"), "hsl(32, 100%, 50%)", "hsl");
  same(uiValFormatColor({ r: 255, g: 0, b: 0, a: 0.5 }, "rgb"), "rgba(255, 0, 0, 0.5)", "rgba");
  same(uiValFormatColor({ r: 255, g: 0, b: 0, a: 0.5 }, "hex"), "#ff000080", "hex alpha");
  same(uiValColorFormat("hsl(1, 2%, 3%)"), "hsl", "format hsl");
  same(uiValColorFormat("rgba(1,2,3,.5)"), "rgb", "format rgb");
  same(uiValColorFormat("#fff"), "hex", "format hex");
});

// M3-INS-11
test("the picker's hue / saturation / value round-trips a colour", () => {
  for (const hex of ["#ff8800", "#3366cc", "#808080", "#000000", "#ffffff", "#12ab9f"]) {
    const c = uiValParseColor(hex)!;
    const [h, s, v] = uiValToHsv(c);
    const back = uiValFromHsv(h!, s!, v!, 1);
    yes(Math.abs(back.r - c.r) <= 1 && Math.abs(back.g - c.g) <= 1 && Math.abs(back.b - c.b) <= 1, `${hex} round trip`);
  }
  const [h, s, v] = uiValToHsv({ r: 0, g: 0, b: 255, a: 1 });
  close(h!, 240, "blue's hue");
  close(s!, 1, "full saturation");
  close(v!, 1, "full value");
});

// M3-INS-11
test("a timing function is found in a transition and written as a keyword when it is one", () => {
  const e = uiValFindEasing("opacity 0.3s ease-in 0.1s")!;
  eq([e.start, e.end, e.curve], [13, 20, [0.42, 0, 1, 1]], "keyword with its place");
  const b = uiValFindEasing("transform 1s cubic-bezier(0.1, 0.7, 1, 0.1)")!;
  eq(b.curve, [0.1, 0.7, 1, 0.1], "cubic-bezier");
  eq(uiValFindEasing("ease-in-out")!.curve, [0.42, 0, 0.58, 1], "the longer keyword wins");
  eq(uiValFindEasing("opacity 1s"), null, "none");
  eq(uiValFindEasing("increase 1s"), null, "a word that contains ease");
  same(uiValFormatEasing([0.25, 0.1, 0.25, 1]), "ease", "a keyword's curve");
  same(uiValFormatEasing([0.1, 0.7, 1, 0.1]), "cubic-bezier(0.1, 0.7, 1, 0.1)", "free curve");
  same(uiValFormatEasing([-1, 0.5, 2, 0.5]), "cubic-bezier(0, 0.5, 1, 0.5)", "x is clamped to 0..1");
  same(uiValFormatEasing([-1, 0, 2, 1]), "linear", "clamping can make a keyword's curve");
});

// M3-INS-11
test("gradients split into a head and stops and come back as text", () => {
  const g = uiValParseGradient("linear-gradient(135deg, #ff5e7e, rgba(0, 0, 0, 0.5) 40%, blue 100%)")!;
  same(g.kind, "linear", "kind");
  same(g.head, "135deg", "head");
  eq(g.stops, [
    { color: "#ff5e7e", pos: "" },
    { color: "rgba(0, 0, 0, 0.5)", pos: "40%" },
    { color: "blue", pos: "100%" },
  ], "stops");
  same(uiValFormatGradient(g), "linear-gradient(135deg, #ff5e7e, rgba(0, 0, 0, 0.5) 40%, blue 100%)", "round trip");
  const plain = uiValParseGradient("linear-gradient(red, blue)")!;
  same(plain.head, "", "no head");
  eq(uiValGradientAngle(plain), 180, "default angle");
  eq(uiValGradientAngle(uiValParseGradient("linear-gradient(to right, red, blue)")!), 90, "to right");
  eq(uiValGradientAngle(g), 135, "degrees");
  const r = uiValParseGradient("radial-gradient(circle, #fff6, #ff5e7e 40%, #3b1d5e)")!;
  same(r.kind, "radial", "radial");
  same(r.head, "circle", "radial head");
  yes(uiValParseGradient("repeating-linear-gradient(45deg, red, blue 10px)")!.repeating, "repeating");
  eq(uiValParseGradient("url(a.png)"), null, "not a gradient");
  eq(uiValSplitTop("a(1, 2), b, c(d(e, f))"), ["a(1, 2)", "b", "c(d(e, f))"], "commas inside brackets stay");
});

// M3-INS-11
test("autocomplete lists prefix matches before inner matches", () => {
  const words = ["flexDirection", "flexGrow", "alignItems", "display", "flex"];
  eq(uiValSuggest("fl", words, 10), ["flexDirection", "flexGrow", "flex"], "prefix");
  eq(uiValSuggest("grow", words, 10), ["flexGrow"], "inner match");
  eq(uiValSuggest("FLEX", words, 2), ["flexDirection", "flexGrow"], "case-insensitive and capped");
  eq(uiValSuggest("flex", words, 10).indexOf("flex"), -1, "the exact word is not suggested");
  eq(uiValSuggest("", words, 10), [], "nothing for an empty prefix");
});

export function run(): string {
  return report();
}
