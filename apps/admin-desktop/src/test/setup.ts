import { configure } from "@testing-library/react";

// jsdom parses its built-in default stylesheet on the first getComputedStyle
// call, and Vitest loads a fresh jsdom for every test file. Role queries call
// getComputedStyle, so without this warm-up each file's first test pays that
// parse (about 0.7 s idle, several seconds under load) inside its own budgets.
getComputedStyle(document.documentElement);

// The 1 s default assumes an unloaded machine. On a busy 8-core machine a single
// query-resolve-and-rerender step has taken almost 4 s. A higher ceiling only
// delays the report of a real failure; passing waits still return immediately.
configure({ asyncUtilTimeout: 10_000 });
