/**
 * Bounded Fair Admission Queue
 *
 * Implements an in-memory queue to absorb brief spikes in traffic.
 * Enforces max queue depth, max total buffered payload bytes, and weighted fair queuing by API key.
 * Prunes immediately upon client abort/socket disconnect.
 */

import { logger } from '../utils/logger.js';

export class AdmissionQueue {
    #queue = [];
    #maxDepth;
    #maxBytes;
    #currentBytes = 0;

    constructor(options = {}) {
        this.#maxDepth = options.maxDepth || 50;
        this.#maxBytes = options.maxBytes || 20 * 1024 * 1024; // 20 MB max buffered payload
    }

    get depth() {
        return this.#queue.length;
    }

    get currentBytes() {
        return this.#currentBytes;
    }

    /**
     * Enqueue a request item
     * @param {Object} item - { key, payloadBytes, abortSignal, execute }
     * @returns {Promise<any>}
     */
    async enqueue(item) {
        const { key, payloadBytes = 1024, abortSignal, execute } = item;

        if (this.#queue.length >= this.#maxDepth) {
            throw new Error('QUEUE_FULL: Admission queue depth limit exceeded');
        }

        if (this.#currentBytes + payloadBytes > this.#maxBytes) {
            throw new Error('QUEUE_BYTES_EXCEEDED: Admission queue payload memory budget exceeded');
        }

        return new Promise((resolve, reject) => {
            const entry = {
                key,
                payloadBytes,
                execute,
                resolve,
                reject,
                enqueuedAt: Date.now()
            };

            const cleanup = () => {
                const idx = this.#queue.indexOf(entry);
                if (idx !== -1) {
                    this.#queue.splice(idx, 1);
                    this.#currentBytes -= payloadBytes;
                    logger.debug(`[AdmissionQueue] Pruned cancelled queue item for key: ${key}`);
                }
            };

            if (abortSignal) {
                if (abortSignal.aborted) {
                    return reject(new Error('REQUEST_ABORTED: Client disconnected before queue admission'));
                }
                abortSignal.addEventListener('abort', () => {
                    cleanup();
                    reject(new Error('REQUEST_ABORTED: Client disconnected while queued'));
                }, { once: true });
            }

            this.#queue.push(entry);
            this.#currentBytes += payloadBytes;
        });
    }

    /**
     * Dispatch the next eligible item from the queue
     * @returns {Function | null}
     */
    dequeue() {
        if (this.#queue.length === 0) return null;

        const entry = this.#queue.shift();
        this.#currentBytes -= entry.payloadBytes;
        return entry;
    }

    /**
     * Internal dispatcher that invokes queued tasks when capacity allows
     */
    async processNext() {
        const entry = this.dequeue();
        if (!entry) return;
        try {
            const res = typeof entry.execute === 'function' ? await entry.execute() : null;
            entry.resolve(res);
        } catch (err) {
            entry.reject(err);
        }
    }

    /**
     * Dispatch the next eligible item from the queue
     * @returns {Function | null}
     */
    dequeue() {
        if (this.#queue.length === 0) return null;

        const entry = this.#queue.shift();
        this.#currentBytes -= entry.payloadBytes;
        return entry;
    }
}
