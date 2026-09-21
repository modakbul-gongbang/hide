import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const hided = process.env.HIDED_ORIGIN ?? "http://127.0.0.1:9876";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/ws": { target: hided, ws: true },
      "/health": hided,
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
