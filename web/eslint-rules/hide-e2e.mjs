// Repository rules for Playwright specs that eslint-plugin-playwright cannot state.
// docs/TESTING.md ("Wait for state, not time", "Writing a Playwright e2e test") owns the reasons.

const ACTIONS = new Set([
  "click", "dblclick", "press", "pressSequentially", "type", "fill", "hover", "focus", "blur",
  "check", "uncheck", "setChecked", "selectOption", "dragTo", "setInputFiles", "tap",
]);

const name = (node) => (node?.type === "Identifier" ? node.name : undefined);

function calleeChain(node) {
  const names = [];
  let current = node;
  while (current) {
    if (current.type === "MemberExpression") {
      const property = name(current.property) ?? (current.property.type === "Literal" ? String(current.property.value) : undefined);
      if (property) names.unshift(property);
      current = current.object;
    } else if (current.type === "CallExpression") current = current.callee;
    else {
      if (current.type === "Identifier") names.unshift(current.name);
      break;
    }
  }
  return names;
}

const isFunction = (node) =>
  node.type === "ArrowFunctionExpression" || node.type === "FunctionExpression" || node.type === "FunctionDeclaration";

/** `expect.poll(fn)` and `expect(fn).toPass()`: the callback is run again until it holds. */
function isRepeatedCallback(fn) {
  const call = fn.parent;
  if (call?.type !== "CallExpression" || !call.arguments.includes(fn)) return false;
  const chain = calleeChain(call.callee);
  if (chain[0] === "expect" && chain.includes("poll") && chain.length <= 3) return true;
  const callee = call.callee;
  return callee.type === "Identifier" && callee.name === "expect" && call.parent?.type === "MemberExpression" && name(call.parent.property) === "toPass";
}

const noActionInPoll = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      action: "A poll only observes: `{{what}}` inside this `expect.poll` or `toPass` is repeated until the assertion holds, so one intent becomes several. Do it once, before the poll (docs/TESTING.md, Wait for state, not time).",
    },
  },
  create(context) {
    const reported = new Set();
    return {
      CallExpression(node) {
        const chain = calleeChain(node.callee);
        const last = chain.at(-1);
        const action = (last && ACTIONS.has(last)) || chain.includes("keyboard") || chain.includes("mouse");
        if (!action) return;
        for (let up = node.parent; up; up = up.parent) {
          if (isFunction(up) && isRepeatedCallback(up)) {
            // One report per poll, on the statement that starts it, so one allow comment covers one poll.
            let poll = up.parent;
            while (poll.parent?.type === "MemberExpression" || (poll.parent?.type === "CallExpression" && poll.parent.callee === poll)) poll = poll.parent;
            if (reported.has(poll)) return;
            reported.add(poll);
            context.report({ node: poll, loc: poll.loc.start, messageId: "action", data: { what: chain.join(".") } });
            return;
          }
        }
      },
    };
  },
};

const text = (context, node) => context.sourceCode.getText(node);

/**
 * A navigation that changes only the URL fragment keeps the old page, and its old token, connected.
 * After `.restart()` in the same function, opening `#token=` has to go through `goto("about:blank")` first.
 * A helper defined elsewhere and called after the restart is not seen; the guide says to keep that step in one helper.
 */
const reopenAfterRestartThroughBlank = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      blank: "After `restart()`, open `#token=` through `goto(\"about:blank\")` first: a navigation that changes only the fragment does not remount the connection, so the old page races the new token (docs/TESTING.md, Writing a Playwright e2e test, step 7).",
    },
  },
  create(context) {
    const frames = [];
    const enter = () => frames.push({ restarted: false });
    const leave = () => frames.pop();
    return {
      ArrowFunctionExpression: enter,
      FunctionExpression: enter,
      FunctionDeclaration: enter,
      "ArrowFunctionExpression:exit": leave,
      "FunctionExpression:exit": leave,
      "FunctionDeclaration:exit": leave,
      CallExpression(node) {
        const frame = frames.at(-1);
        if (!frame) return;
        const method = name(node.callee?.property);
        if (method === "restart") frame.restarted = true;
        if (method !== "goto" || !frame.restarted) return;
        const url = node.arguments[0];
        if (!url) return;
        if (url.type === "Literal" && url.value === "about:blank") frame.restarted = false;
        else if (text(context, url).includes("#token=")) context.report({ node, messageId: "blank" });
      },
    };
  },
};

const WINDOW_SIZING = new Set(["setSize", "setBounds", "setContentSize", "setContentBounds"]);

/**
 * macOS keeps a window larger than the screen's work area until the window is next ordered in (a `blur()` or
 * `focus()` of the test's own) and clamps it then, so the layout changes in the middle of the test (issue 511).
 * A desktop spec sizes its window through `fitWindow` in `desktop/e2e/fixture.ts`, the one place that keeps it inside
 * the work area; a spec that needs a window larger than the screen says why in a line allow.
 */
const windowSizeThroughFixture = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      fixture: "Size the window with `fitWindow` from desktop/e2e/fixture.ts, not `{{method}}`: macOS clamps a window larger than the work area when it is next ordered in, which changes the layout mid-test (docs/TESTING.md, Writing a Playwright e2e test, step 1).",
    },
  },
  create(context) {
    if (context.filename.replaceAll("\\", "/").endsWith("/desktop/e2e/fixture.ts")) return {};
    return {
      CallExpression(node) {
        const method = name(node.callee?.property);
        if (node.callee?.type === "MemberExpression" && WINDOW_SIZING.has(method)) context.report({ node, messageId: "fixture", data: { method } });
      },
    };
  },
};

export default {
  rules: {
    "no-action-in-poll": noActionInPoll,
    "reopen-after-restart-through-blank": reopenAfterRestartThroughBlank,
    "window-size-through-fixture": windowSizeThroughFixture,
  },
};
