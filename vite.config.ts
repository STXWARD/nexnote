import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({ plugins: [react()], clearScreen: false, server: { port: 1420, strictPort: true }, build: { rollupOptions: { output: { manualChunks(id) { if (id.includes("/node_modules/@tiptap/") || id.includes("/node_modules/lowlight/") || id.includes("/node_modules/highlight.js/")) return "rich-editor"; if (id.includes("/node_modules/react") || id.includes("/node_modules/scheduler/")) return "react-vendor"; } } } } });
