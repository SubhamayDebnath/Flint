import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // prevent Vite from obscuring Rust errors
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // never watch Rust build output (causes EBUSY on Windows)
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: { target: "chrome105", sourcemap: false },
});
