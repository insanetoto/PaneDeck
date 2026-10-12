import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    css: { include: [/tokens\.css/] },
    setupFiles: ["./src/test/setup.ts"],
  },
});
