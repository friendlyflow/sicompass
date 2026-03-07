// FFON Server SDK
// A Bun HTTP server that serves FFON content for sicompass remote providers.
//
// Usage:
//   import { createFfonServer } from "./ffon-server";
//
//   createFfonServer({
//     port: 3000,
//     apiKeys: ["sk-paid-user-123"],
//     fetch(path) {
//       return ["Hello from my FFON service"];
//     },
//   });

export type FfonElement = string | { [key: string]: FfonElement[] };

export interface KeyStore {
  createKey(customerId: string): string;
  revokeKey(customerId: string): void;
  isValidKey(apiKey: string): boolean;
  getKeyForCustomer(customerId: string): string | null;
}

export interface FfonServerOps {
  // Port to listen on (default: 3000)
  port?: number;

  // Valid API keys. If empty/undefined, no auth required.
  apiKeys?: string[];

  // Dynamic key store (e.g., managed by Stripe/LemonSqueezy billing).
  // Checked alongside apiKeys — either source can authorize a request.
  keyStore?: KeyStore;

  // Route handler: given a URL path, return FFON elements.
  // The response can include <link> tagged objects for sub-navigation.
  fetch(path: string): FfonElement[] | Promise<FfonElement[]>;

  // Optional: handle non-FFON routes (billing, webhooks, etc.)
  // Return a Response to handle the route, or null to fall through to FFON handler.
  handleRoute?(request: Request, url: URL): Promise<Response | null>;
}

export function createFfonServer(ops: FfonServerOps): void {
  const port = ops.port ?? 3000;

  function validateAuth(request: Request): boolean {
    const noStaticKeys = !ops.apiKeys || ops.apiKeys.length === 0;
    const noKeyStore = !ops.keyStore;
    if (noStaticKeys && noKeyStore) return true;

    const auth = request.headers.get("Authorization");
    if (!auth) return false;
    const token = auth.startsWith("Bearer ") ? auth.slice(7) : "";
    if (!token) return false;

    if (ops.apiKeys?.includes(token)) return true;
    if (ops.keyStore?.isValidKey(token)) return true;
    return false;
  }

  Bun.serve({
    port,
    async fetch(request: Request): Promise<Response> {
      const headers = {
        "Content-Type": "application/json",
        "Access-Control-Allow-Origin": "*",
        "Access-Control-Allow-Headers": "Authorization, Content-Type",
      };

      if (request.method === "OPTIONS") {
        return new Response(null, { status: 204, headers });
      }

      const url = new URL(request.url);

      // Let custom route handler try first (billing endpoints, etc.)
      if (ops.handleRoute) {
        const customResponse = await ops.handleRoute(request, url);
        if (customResponse) return customResponse;
      }

      // Auth check for FFON content routes
      if (!validateAuth(request)) {
        return new Response(JSON.stringify({ error: "Unauthorized" }), {
          status: 401,
          headers,
        });
      }

      const path = decodeURIComponent(url.pathname);

      try {
        const elements = await ops.fetch(path);
        return new Response(JSON.stringify(elements), { status: 200, headers });
      } catch (err) {
        const message =
          err instanceof Error ? err.message : "Internal server error";
        return new Response(JSON.stringify({ error: message }), {
          status: 500,
          headers,
        });
      }
    },
  });

  console.log(`FFON server listening on http://localhost:${port}`);
}
