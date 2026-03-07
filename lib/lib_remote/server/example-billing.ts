// Example: FFON server with Stripe + LemonSqueezy billing (round-robin)
//
// Setup:
//   1. bun add stripe
//   2. Set environment variables (see below)
//   3. bun run example-billing.ts
//   4. Configure Stripe webhook: POST https://your-domain.com/billing/webhook/stripe
//   5. Configure LemonSqueezy webhook: POST https://your-domain.com/billing/webhook/lemonsqueezy
//
// Environment variables:
//   STRIPE_SECRET_KEY        - Stripe secret key (sk_test_... or sk_live_...)
//   STRIPE_WEBHOOK_SECRET    - Stripe webhook signing secret (whsec_...)
//   STRIPE_PRICE_ID          - Stripe subscription price ID (price_...)
//   LEMONSQUEEZY_API_KEY     - LemonSqueezy API key
//   LEMONSQUEEZY_WEBHOOK_SECRET - LemonSqueezy webhook signing secret
//   LEMONSQUEEZY_STORE_ID    - LemonSqueezy store ID
//   LEMONSQUEEZY_VARIANT_ID  - LemonSqueezy subscription variant ID
//
// Checkout flow:
//   POST /billing/checkout  → returns { url } → redirect user to pay
//   Each checkout alternates between Stripe and LemonSqueezy.
//   On payment: webhook fires → API key generated → shown on success page.
//
// User adds API key to ~/.config/sicompass/settings.json:
//   "my premium service": {
//     "remoteUrl": "https://your-domain.com",
//     "apiKey": "sk-<generated-key>"
//   }

import { createBillingServer } from "./billing-server";

createBillingServer({
  port: 3000,
  successUrl: "https://example.com/success",
  cancelUrl: "https://example.com/cancel",

  stripe: {
    secretKey: process.env.STRIPE_SECRET_KEY!,
    webhookSecret: process.env.STRIPE_WEBHOOK_SECRET!,
    priceId: process.env.STRIPE_PRICE_ID!,
  },

  lemonSqueezy: {
    apiKey: process.env.LEMONSQUEEZY_API_KEY!,
    webhookSecret: process.env.LEMONSQUEEZY_WEBHOOK_SECRET!,
    storeId: process.env.LEMONSQUEEZY_STORE_ID!,
    variantId: process.env.LEMONSQUEEZY_VARIANT_ID!,
  },

  fetch(path) {
    if (path === "/root") {
      return [
        "Welcome to the Premium FFON Service",
        {
          "<link>http://localhost:3000/dashboard</link>Dashboard": [],
        },
        {
          "<link>http://localhost:3000/data</link>Premium Data": [],
        },
      ];
    }

    if (path === "/dashboard") {
      return [
        { "Usage This Month": ["API calls: 1,247", "Data served: 15.2 MB"] },
        {
          "Account": [
            "Status: Active",
            "Plan: Monthly",
            "Next billing: April 7, 2026",
          ],
        },
      ];
    }

    if (path === "/data") {
      return [
        { "Dataset A": ["Record 1", "Record 2", "Record 3"] },
        { "Dataset B": ["Item alpha", "Item beta", "Item gamma"] },
      ];
    }

    return [`No content at path: ${path}`];
  },
});
