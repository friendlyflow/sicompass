// Billing infrastructure: PaymentProvider interface, KeyStore, and BillingRouter
//
// Shared abstractions for Stripe, LemonSqueezy, or any payment provider.

import { randomBytes } from "crypto";
import { readFileSync, writeFileSync, existsSync } from "fs";
import type { KeyStore } from "./ffon-server";

export type WebhookEvent =
  | "subscription.created"
  | "subscription.deleted"
  | "payment.failed"
  | "unknown";

export interface WebhookResult {
  event: WebhookEvent;
  customerId: string;
}

export interface PaymentProvider {
  name: string;
  createCheckoutUrl(successUrl: string, cancelUrl: string): Promise<string>;
  handleWebhook(request: Request): Promise<WebhookResult>;
  getPortalUrl(customerId: string): Promise<string | null>;
}

// JSON file-based key store
interface KeyRecord {
  apiKey: string;
  provider: string;
  createdAt: string;
}

interface KeyStoreData {
  customers: Record<string, KeyRecord>;
}

export class FileKeyStore implements KeyStore {
  private data: KeyStoreData;
  private filePath: string;

  constructor(filePath: string = "./keys.json") {
    this.filePath = filePath;
    if (existsSync(filePath)) {
      this.data = JSON.parse(readFileSync(filePath, "utf-8"));
    } else {
      this.data = { customers: {} };
    }
  }

  private save(): void {
    writeFileSync(this.filePath, JSON.stringify(this.data, null, 2));
  }

  createKey(customerId: string): string {
    const existing = this.data.customers[customerId];
    if (existing) return existing.apiKey;

    const apiKey = `sk-${randomBytes(24).toString("hex")}`;
    this.data.customers[customerId] = {
      apiKey,
      provider: "",
      createdAt: new Date().toISOString(),
    };
    this.save();
    return apiKey;
  }

  createKeyWithProvider(customerId: string, providerName: string): string {
    const existing = this.data.customers[customerId];
    if (existing) return existing.apiKey;

    const apiKey = `sk-${randomBytes(24).toString("hex")}`;
    this.data.customers[customerId] = {
      apiKey,
      provider: providerName,
      createdAt: new Date().toISOString(),
    };
    this.save();
    return apiKey;
  }

  revokeKey(customerId: string): void {
    delete this.data.customers[customerId];
    this.save();
  }

  isValidKey(apiKey: string): boolean {
    return Object.values(this.data.customers).some(
      (record) => record.apiKey === apiKey,
    );
  }

  getKeyForCustomer(customerId: string): string | null {
    return this.data.customers[customerId]?.apiKey ?? null;
  }
}

// Round-robin billing router
export class BillingRouter {
  private providers: PaymentProvider[];
  private keyStore: FileKeyStore;
  private index: number = 0;

  constructor(providers: PaymentProvider[], keyStore: FileKeyStore) {
    this.providers = providers;
    this.keyStore = keyStore;
  }

  async nextCheckoutUrl(
    successUrl: string,
    cancelUrl: string,
  ): Promise<string> {
    const provider = this.providers[this.index % this.providers.length];
    this.index++;
    return provider.createCheckoutUrl(successUrl, cancelUrl);
  }

  async handleWebhook(
    providerName: string,
    request: Request,
  ): Promise<{ event: WebhookEvent; apiKey?: string }> {
    const provider = this.providers.find((p) => p.name === providerName);
    if (!provider) {
      return { event: "unknown" };
    }

    const result = await provider.handleWebhook(request);

    if (result.event === "subscription.created" && result.customerId) {
      const apiKey = this.keyStore.createKeyWithProvider(
        result.customerId,
        providerName,
      );
      return { event: result.event, apiKey };
    }

    if (
      result.event === "subscription.deleted" ||
      result.event === "payment.failed"
    ) {
      this.keyStore.revokeKey(result.customerId);
      return { event: result.event };
    }

    return { event: result.event };
  }

  getProviderForCustomer(customerId: string): PaymentProvider | null {
    // Look up which provider this customer used
    const record = (this.keyStore as any).data?.customers?.[customerId];
    if (!record) return null;
    return this.providers.find((p) => p.name === record.provider) ?? null;
  }
}
