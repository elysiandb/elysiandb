import { Editor, loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import TypeScriptWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

self.MonacoEnvironment = {
    getWorker(_, label) {
        if (label === "typescript" || label === "javascript") {
            return new TypeScriptWorker();
        }

        return new EditorWorker();
    },
};

loader.config({ monaco });

export default Editor;
