// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://combine-lab.github.io",
  base: "/simpleaf",
  integrations: [
    starlight({
      title: "simpleaf",
      description:
        "simpleaf simplifies and customizes single-cell processing with piscem and alevin-fry.",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/COMBINE-lab/simpleaf",
        },
      ],
      editLink: {
        baseUrl:
          "https://github.com/COMBINE-lab/simpleaf/edit/main/docs-site/",
      },
      sidebar: [
        {
          label: "Getting started",
          items: [
            { label: "Welcome", link: "/" },
            { label: "Installation", slug: "installation" },
            { label: "set-paths", slug: "set-paths" },
            { label: "refresh-prog-info", slug: "refresh-prog-info" },
          ],
        },
        {
          label: "Commands",
          items: [
            { label: "index", slug: "index-command" },
            { label: "quant", slug: "quant-command" },
            { label: "flex-quant", slug: "flex-quant-command" },
            { label: "atac process", slug: "atac-process-command" },
            { label: "chemistry", slug: "chemistry-command" },
            { label: "inspect", slug: "inspect-command" },
          ],
        },
        {
          label: "Guides",
          items: [
            {
              label: "Threads and decompression",
              slug: "threads-and-decompression",
            },
          ],
        },
        {
          label: "Workflows",
          items: [
            { label: "Overview", slug: "workflow" },
            { label: "workflow get", slug: "workflow-get" },
            { label: "workflow run", slug: "workflow-run" },
            { label: "workflow list", slug: "workflow-list" },
            { label: "workflow patch", slug: "workflow-patch" },
            { label: "workflow refresh", slug: "workflow-refresh" },
            { label: "Utility library", slug: "workflow-utility-library" },
          ],
        },
        { label: "License", slug: "license" },
      ],
    }),
  ],
});
