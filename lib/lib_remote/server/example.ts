// Example FFON server - demonstrates how to create a paid FFON service
//
// Run with: bun run example.ts
// Then add to ~/.config/sicompass/settings.json:
//   "programsToLoad": ["file browser", "example service"]
//   "example service": {
//     "remoteUrl": "http://localhost:3000",
//     "apiKey": "sk-demo-key"
//   }

import { createFfonServer } from "./ffon-server";

createFfonServer({
  port: 3000,
  apiKeys: ["sk-demo-key"],

  fetch(path) {
    if (path === "/root") {
      return [
        "Welcome to the Example FFON Service",
        {
          "<link>http://localhost:3000/products</link>Products": [],
        },
        {
          "<link>http://localhost:3000/analytics</link>Analytics Dashboard": [],
        },
        "Tip: navigate right into any section to load remote data",
      ];
    }

    if (path === "/products") {
      return [
        { "Widget Pro": ["$29.99", "In stock: 142", "SKU: WP-001"] },
        { "Gadget Plus": ["$49.99", "In stock: 87", "SKU: GP-002"] },
        {
          "Super Bundle": [
            "$69.99",
            "In stock: 23",
            "SKU: SB-003",
            {
              "<link>http://localhost:3000/products/super-bundle/details</link>Bundle Contents":
                [],
            },
          ],
        },
      ];
    }

    if (path === "/products/super-bundle/details") {
      return [
        "1x Widget Pro",
        "1x Gadget Plus",
        "1x Premium Case",
        "Free shipping included",
      ];
    }

    if (path === "/analytics") {
      return [
        { "Today": ["Orders: 47", "Revenue: $2,341", "Visitors: 1,205"] },
        {
          "This Week": ["Orders: 312", "Revenue: $15,678", "Visitors: 8,432"],
        },
        {
          "This Month": [
            "Orders: 1,247",
            "Revenue: $62,341",
            "Visitors: 34,521",
          ],
        },
      ];
    }

    return [`No content at path: ${path}`];
  },
});
