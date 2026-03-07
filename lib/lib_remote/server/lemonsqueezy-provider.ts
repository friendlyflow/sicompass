// LemonSqueezy payment provider implementation
// Uses plain fetch() — no SDK dependency needed.

import { createHmac } from "crypto";
import type { PaymentProvider, WebhookResult } from "./billing";

export interface LemonSqueezyConfig {
  apiKey: string;
  webhookSecret: string;
  storeId: string;
  variantId: string;
}

const LS_API = "https://api.lemonsqueezy.com/v1";

export function createLemonSqueezyProvider(
  config: LemonSqueezyConfig,
): PaymentProvider {
  async function lsFetch(
    path: string,
    options: RequestInit = {},
  ): Promise<any> {
    const response = await fetch(`${LS_API}${path}`, {
      ...options,
      headers: {
        Authorization: `Bearer ${config.apiKey}`,
        Accept: "application/vnd.api+json",
        "Content-Type": "application/vnd.api+json",
        ...(options.headers ?? {}),
      },
    });
    if (!response.ok) {
      throw new Error(
        `LemonSqueezy API error: ${response.status} ${response.statusText}`,
      );
    }
    return response.json();
  }

  function verifySignature(body: string, signature: string): boolean {
    const hmac = createHmac("sha256", config.webhookSecret);
    hmac.update(body);
    const digest = hmac.digest("hex");
    return digest === signature;
  }

  return {
    name: "lemonsqueezy",

    async createCheckoutUrl(
      successUrl: string,
      cancelUrl: string,
    ): Promise<string> {
      const data = await lsFetch("/checkouts", {
        method: "POST",
        body: JSON.stringify({
          data: {
            type: "checkouts",
            attributes: {
              checkout_data: {
                custom: {},
              },
              product_options: {
                redirect_url: successUrl,
              },
            },
            relationships: {
              store: { data: { type: "stores", id: config.storeId } },
              variant: { data: { type: "variants", id: config.variantId } },
            },
          },
        }),
      });
      return data.data.attributes.url;
    },

    async handleWebhook(request: Request): Promise<WebhookResult> {
      const body = await request.text();
      const signature = request.headers.get("x-signature");
      if (!signature || !verifySignature(body, signature)) {
        return { event: "unknown", customerId: "" };
      }

      let payload: any;
      try {
        payload = JSON.parse(body);
      } catch {
        return { event: "unknown", customerId: "" };
      }

      const eventName = payload.meta?.event_name;
      const customerId = String(
        payload.data?.attributes?.customer_id ?? payload.meta?.custom_data?.customer_id ?? "",
      );

      switch (eventName) {
        case "subscription_created":
          return { event: "subscription.created", customerId };
        case "subscription_expired":
        case "subscription_cancelled":
          return { event: "subscription.deleted", customerId };
        case "subscription_payment_failed":
          return { event: "payment.failed", customerId };
        default:
          return { event: "unknown", customerId: "" };
      }
    },

    async getPortalUrl(customerId: string): Promise<string | null> {
      try {
        const data = await lsFetch(`/customers/${customerId}`);
        return data.data?.attributes?.urls?.customer_portal ?? null;
      } catch {
        return null;
      }
    },
  };
}
