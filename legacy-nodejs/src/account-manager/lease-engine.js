/**
 * Attempt Lease Engine
 *
 * Provides atomic synchronous select-and-reserve attempt leases for accounts.
 * Tracks active in-flight executions per account and releases reliably and idempotently.
 */

import { logger } from '../utils/logger.js';
import { resolveModel } from '../constants.js';

export class LeaseEngine {
    #inFlightCounts = new Map(); // email -> number
    #maxPerAccount;
    #maxTotal;

    constructor(options = {}) {
        this.#maxPerAccount = options.maxPerAccount || 2;
        this.#maxTotal = options.maxTotal || 20;
    }

    /**
     * Get active in-flight count for an account
     * @param {string} email
     * @returns {number}
     */
    getInFlight(email) {
        return this.#inFlightCounts.get(email) || 0;
    }

    /**
     * Get total active in-flight count across all accounts
     * @returns {number}
     */
    getTotalInFlight() {
        let total = 0;
        for (const count of this.#inFlightCounts.values()) {
            total += count;
        }
        return total;
    }

    /**
     * Atomically acquire an attempt lease on an account
     * @param {Array} accounts - Configured accounts pool
     * @param {string} modelId - Model ID
     * @returns {{ account: Object, lease: Object } | null}
     */
    acquireLease(accounts, modelId) {
        if (this.getTotalInFlight() >= this.#maxTotal) {
            logger.warn(`[LeaseEngine] Gateway total concurrency ceiling (${this.#maxTotal}) reached`);
            return null;
        }

        const canonicalModel = resolveModel(modelId);
        const now = Date.now();

        // Filter eligible accounts: enabled, valid, not rate-limited for this model, and under max per-account
        const eligible = accounts.filter(acc => {
            if (acc.enabled === false) return false;
            if (acc.isInvalid) return false;

            const inFlight = this.getInFlight(acc.email);
            if (inFlight >= this.#maxPerAccount) return false;

            if (canonicalModel && acc.modelRateLimits && acc.modelRateLimits[canonicalModel]) {
                const limit = acc.modelRateLimits[canonicalModel];
                if (limit.isRateLimited && limit.resetTime > now) return false;
            }

            return true;
        });

        if (eligible.length === 0) {
            return null;
        }

        // Balance by:
        // 1) Least In-Flight count
        // 2) Highest Remaining Quota Fraction (if available)
        // 3) Least Recently Used
        eligible.sort((a, b) => {
            const infA = this.getInFlight(a.email);
            const infB = this.getInFlight(b.email);
            if (infA !== infB) return infA - infB;

            const fracA = (canonicalModel && a.modelQuotas && a.modelQuotas[canonicalModel]?.remainingFraction != null)
                ? a.modelQuotas[canonicalModel].remainingFraction
                : 1.0;
            const fracB = (canonicalModel && b.modelQuotas && b.modelQuotas[canonicalModel]?.remainingFraction != null)
                ? b.modelQuotas[canonicalModel].remainingFraction
                : 1.0;

            if (fracA !== fracB) return fracB - fracA; // Higher remaining fraction wins

            return (a.lastUsed || 0) - (b.lastUsed || 0);
        });

        const selected = eligible[0];
        const currentCount = this.getInFlight(selected.email);
        this.#inFlightCounts.set(selected.email, currentCount + 1);
        selected.lastUsed = Date.now();

        let released = false;
        const lease = {
            account: selected,
            email: selected.email,
            acquiredAt: Date.now(),
            release: () => {
                if (released) return;
                released = true;
                const c = this.getInFlight(selected.email);
                if (c <= 1) {
                    this.#inFlightCounts.delete(selected.email);
                } else {
                    this.#inFlightCounts.set(selected.email, c - 1);
                }
            }
        };

        return { account: selected, lease };
    }
}
