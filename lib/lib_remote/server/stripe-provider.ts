// Stripe payment provider implementation

import Stripe from "stripe";
import type { PaymentProvider, WebhookResult } from "./billing";

export interface StripeConfig {
  secretKey: string;
  webhookSecret: string;
  priceId: string;
}

export function createStripeProvider(config: StripeConfig): PaymentProvider {
  const stripe = new Stripe(config.secretKey);

  return {
    name: "stripe",

    async createCheckoutUrl(
      successUrl: string,
      cancelUrl: string,
    ): Promise<string> {
      const session = await stripe.checkout.sessions.create({
        mode: "subscription",
        line_items: [{ price: config.priceId, quantity: 1 }],
        success_url: successUrl,
        cancel_url: cancelUrl,
      });
      return session.url!;
    },

    async handleWebhook(request: Request): Promise<WebhookResult> {
      const body = await request.text();
      const signature = request.headers.get("stripe-signature");
      if (!signature) {
        return { event: "unknown", customerId: "" };
      }

      let event: Stripe.Event;
      try {
        event = stripe.webhooks.constructEvent(
          body,
          signature,
          config.webhookSecret,
        );
      } catch {
        return { event: "unknown", customerId: "" };
      }

      switch (event.type) {
        case "checkout.session.completed": {
          const session = event.data.object as Stripe.Checkout.Session;
          return {
            event: "subscription.created",
            customerId: session.customer as string,
          };
        }
        case "customer.subscription.deleted": {
          const subscription = event.data.object as Stripe.Subscription;
          return {
            event: "subscription.deleted",
            customerId: subscription.customer as string,
          };
        }
        case "invoice.payment_failed": {
          const invoice = event.data.object as Stripe.Invoice;
          return {
            event: "payment.failed",
            customerId: invoice.customer as string,
          };
        }
        default:
          return { event: "unknown", customerId: "" };
      }
    },

    async getPortalUrl(customerId: string): Promise<string | null> {
      try {
        const session = await stripe.billingPortal.sessions.create({
          customer: customerId,
        });
        return session.url;
      } catch {
        return null;
      }
    },
  };
}
