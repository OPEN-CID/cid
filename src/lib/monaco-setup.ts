// `@monaco-editor/react` defaults to fetching Monaco from cdn.jsdelivr.net at
// runtime. That made the Editor tab silently dependent on public internet — it
// showed "Loading..." forever offline, behind a proxy, or under a strict CSP —
// in a self-hosted tool whose whole point is running against your own machine.
// It also served a *different build* than the one we bundle (CDN 0.55.1 vs the
// installed 0.56.0), and pulled third-party script into a page that has your
// source open.
//
// Importing the local package and handing it to `loader.config` pins the editor
// to the version in package-lock.json and makes the tab work with the network
// off. This module is imported for its side effects and must run before the
// first <Editor> renders; EditorPane is lazy-loaded, so Monaco still stays out
// of the entry bundle.
import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";
// Specifiers deliberately omit `esm/vs/`: monaco-editor 0.56 ships an
// `exports` map of `"./*": "./esm/vs/*.js"`, so the conventional
// `monaco-editor/esm/vs/...` path from older guides resolves to
// `esm/vs/esm/vs/...` and fails.
import editorWorker from "monaco-editor/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/language/json/json.worker?worker";
import cssWorker from "monaco-editor/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/language/html/html.worker?worker";
import tsWorker from "monaco-editor/language/typescript/ts.worker?worker";

// Monaco reads this global to spawn language workers. Without it the workers
// are requested over the network too, which is the same failure one layer down.
(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    switch (label) {
      case "json":
        return new jsonWorker();
      case "css":
      case "scss":
      case "less":
        return new cssWorker();
      case "html":
      case "handlebars":
      case "razor":
        return new htmlWorker();
      case "typescript":
      case "javascript":
        return new tsWorker();
      default:
        return new editorWorker();
    }
  },
};

loader.config({ monaco });
