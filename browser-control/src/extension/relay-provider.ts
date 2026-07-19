import type { RelayConnection } from '../index.js';

export type AuthorizedRelayConsumer = (relay: RelayConnection) => void;

/** Composition boundary used by the loopback adapter after it authorizes a relay. */
export class AuthorizedRelayProvider {
  #consumer: AuthorizedRelayConsumer | undefined;

  subscribe(consumer: AuthorizedRelayConsumer): () => void {
    if (this.#consumer)
      throw new Error('An authorized relay consumer is already installed');
    this.#consumer = consumer;
    return () => {
      if (this.#consumer === consumer)
        this.#consumer = undefined;
    };
  }

  accept(relay: RelayConnection): void {
    const consumer = this.#consumer;
    if (!consumer) {
      relay.close();
      return;
    }
    consumer(relay);
  }
}

export const authorizedRelayProvider = new AuthorizedRelayProvider();
