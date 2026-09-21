import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import fs from "node:fs";

const runDir =
  process.env.S0_RUN_DIR ??
  path.resolve(import.meta.dirname, "../../../agents/runs/web-shell-pivot-s0");

export default defineConfig({
  plugins: [
    react(),
    {
      name: "s0-capture",
      configureServer(server) {
        server.middlewares.use((req, res, next) => {
          if (req.url === "/capture.jsonl") {
            const file = path.join(runDir, "capture.jsonl");
            if (!fs.existsSync(file)) {
              res.statusCode = 404;
              res.end("capture.jsonl missing");
              return;
            }
            res.setHeader("content-type", "application/x-ndjson");
            fs.createReadStream(file).pipe(res);
            return;
          }
          next();
        });
      },
    },
  ],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    proxy: {
      "/ws": {
        target: "ws://127.0.0.1:9876",
        ws: true,
      },
    },
  },
});
