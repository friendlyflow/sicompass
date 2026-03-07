// Billing server: wires Stripe + LemonSqueezy billing with the FFON server.
// Handles checkout, webhooks, and customer portal routes.

import { createFfonServer, type FfonElement } from "./ffon-server";
import { BillingRouter, FileKeyStore } from "./billing";
import {
  createStripeProvider,
  type StripeConfig,
} from "./stripe-provider";
import {
  createLemonSqueezyProvider,
  type LemonSqueezyConfig,
} from "./lemonsqueezy-provider";

export interface BillingServerOps {
  port?: number;
  stripe: StripeConfig;
  lemonSqueezy: LemonSqueezyConfig;
  successUrl: string;
  cancelUrl: string;
  keyStorePath?: string;
  fetch(path: string): FfonElement[] | Promise<FfonElement[]>;
}

export function createBillingServer(ops: BillingServerOps): void {
  const keyStore = new FileKeyStore(ops.keyStorePath ?? "./keys.json");

  const stripeProvider = createStripeProvider(ops.stripe);
  const lemonSqueezyProvider = createLemonSqueezyProvider(ops.lemonSqueezy);
  const router = new BillingRouter(
    [stripeProvider, lemonSqueezyProvider],
    keyStore,
  );

  const headers = {
    "Content-Type": "application/json",
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Headers": "Authorization, Content-Type",
  };

  createFfonServer({
    port: ops.port,
    keyStore,
    fetch: ops.fetch,

    async handleRoute(
      request: Request,
      url: URL,
    ): Promise<Response | null> {
      const path = url.pathname;

      // Create checkout session (round-robin between providers)
      if (path === "/billing/checkout" && request.method === "POST") {
        try {
          const checkoutUrl = await router.nextCheckoutUrl(
            ops.successUrl,
            ops.cancelUrl,
          );
          return new Response(JSON.stringify({ url: checkoutUrl }), {
            status: 200,
            headers,
          });
        } catch (err) {
          const message =
            err instanceof Error ? err.message : "Checkout failed";
          return new Response(JSON.stringify({ error: message }), {
            status: 500,
            headers,
          });
        }
      }

      // Stripe webhook
      if (
        path === "/billing/webhook/stripe" &&
        request.method === "POST"
      ) {
        try {
          const result = await router.handleWebhook("stripe", request);
          if (result.apiKey) {
            console.log(
              `[stripe] New subscription — API key: ${result.apiKey}`,
            );
          } else if (result.event === "subscription.deleted") {
            console.log("[stripe] Subscription cancelled — key revoked");
          }
          return new Response(JSON.stringify({ received: true }), {
            status: 200,
            headers,
          });
        } catch {
          return new Response(
            JSON.stringify({ error: "Webhook processing failed" }),
            { status: 400, headers },
          );
        }
      }

      // LemonSqueezy webhook
      if (
        path === "/billing/webhook/lemonsqueezy" &&
        request.method === "POST"
      ) {
        try {
          const result = await router.handleWebhook(
            "lemonsqueezy",
            request,
          );
          if (result.apiKey) {
            console.log(
              `[lemonsqueezy] New subscription — API key: ${result.apiKey}`,
            );
          } else if (result.event === "subscription.deleted") {
            console.log(
              "[lemonsqueezy] Subscription cancelled — key revoked",
            );
          }
          return new Response(JSON.stringify({ received: true }), {
            status: 200,
            headers,
          });
        } catch {
          return new Response(
            JSON.stringify({ error: "Webhook processing failed" }),
            { status: 400, headers },
          );
        }
      }

      // Customer portal redirect
      if (path === "/billing/portal" && request.method === "GET") {
        const customerId = url.searchParams.get("customer");
        if (!customerId) {
          return new Response(
            JSON.stringify({ error: "customer parameter required" }),
            { status: 400, headers },
          );
        }
        const provider = router.getProviderForCustomer(customerId);
        if (!provider) {
          return new Response(
            JSON.stringify({ error: "Customer not found" }),
            { status: 404, headers },
          );
        }
        const portalUrl = await provider.getPortalUrl(customerId);
        if (!portalUrl) {
          return new Response(
            JSON.stringify({ error: "Portal unavailable" }),
            { status: 500, headers },
          );
        }
        return Response.redirect(portalUrl, 302);
      }

      // Not a billing route — fall through to FFON handler
      return null;
    },
  });
}
