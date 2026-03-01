#pragma once

#include <stdbool.h>

typedef struct {
    bool success;
    char accessToken[2048];
    char refreshToken[2048];
    long expiresIn;
    char error[512];
} OAuth2TokenResult;

/**
 * Start the OAuth2 authorization flow for Google.
 * Opens browser for Google login, waits for redirect on a local HTTP server,
 * then exchanges the authorization code for tokens.
 */
OAuth2TokenResult emailclientOAuth2Authorize(const char *clientId,
                                              const char *clientSecret,
                                              int timeoutSeconds);

/**
 * Refresh an expired access token using a refresh token.
 */
OAuth2TokenResult emailclientOAuth2RefreshToken(const char *clientId,
                                                 const char *clientSecret,
                                                 const char *refreshToken);
