/**
 * The postMessage protocol between an app and the Lowki wallet frame.
 *
 * Mirrors `apps/wallet-ui/src/protocol.rs` (`DappToWallet` / `WalletToDapp`).
 * Every message is `{ lowki: 1, type: "<snake_case>", ...fields }`. Requests
 * carry an `id` and the wallet echoes it in the answer.
 *
 * Fields marked "proposed" are not in protocol v1 yet. The SDK sends or reads
 * them only when the wallet advertises support, and tolerates their absence.
 */

export const PROTOCOL_VERSION = 1 as const;

export type Hex = `0x${string}`;

/** `TxRequest` in the wallet: `value` is wei as a decimal string. */
export interface WireTxRequest {
  to: string;
  value: string;
  data: string;
}

export type DelegationRule =
  | { kind: 'swap'; router: string; router_label?: string; token: string; max_per_day: string }
  | { kind: 'call'; contract: string; contract_label?: string; function: string; max_per_day?: string }
  | { kind: 'transfer'; to: string; token: string; max_per_day: string };

export interface DelegationPolicy {
  chain: string;
  /** YYYY-MM-DD */
  expires: string;
  rules: DelegationRule[];
}

/** App -> wallet. */
export type DappToWallet =
  | { type: 'sign_in'; id: string }
  | { type: 'sign_message'; id: string; message: string }
  | { type: 'sign_typed_data'; id: string; typed_data: unknown }
  | { type: 'send_transaction'; id: string; tx: WireTxRequest }
  | { type: 'request_delegation'; id: string; policy: DelegationPolicy }
  | { type: 'sign_out'; id: string }
  /** Proposed (not in protocol v1): a private transfer through Privacy Pools. */
  | { type: 'send_privately'; id: string; to: string; amount: string; asset: string };

export type DappRequestType = DappToWallet['type'];

export type RejectReason = 'user_rejected' | 'policy_rejected' | 'not_signed_in' | 'network_error' | 'busy';

/** Wallet -> app. */
export type WalletToDapp =
  | {
      type: 'ready';
      app_id: string;
      /** Proposed: request types this wallet build understands. Absent in v1. */
      capabilities?: string[];
    }
  | {
      type: 'authenticated';
      id?: string | null;
      address: string;
      user_id: string;
      access_token: string;
      /** Proposed: only present when the user chose to share it with this app. */
      email?: string;
      /** Proposed: ISO 8601 time the pairwise account was created. */
      created_at?: string;
      /** Proposed: true on the first sign-in to this app. */
      is_new_user?: boolean;
    }
  | { type: 'message_signed'; id: string; signature: string }
  | { type: 'typed_data_signed'; id: string; signature: string }
  | { type: 'transaction_sent'; id: string; hash: string; sponsored: boolean }
  /** Proposed answer to `send_privately`. `transaction_sent` is accepted too. */
  | { type: 'private_sent'; id: string; hash: string }
  | { type: 'delegation_approved'; id: string; delegation_id: string; expires: string; attestation_build: string }
  | { type: 'delegation_revoked'; delegation_id: string }
  | { type: 'rejected'; id: string; reason: RejectReason; message: string }
  | { type: 'signed_out'; id?: string | null };

export type WalletMessageType = WalletToDapp['type'];

/** Request types every v1 wallet understands (protocol.rs `DappToWallet`). */
export const V1_CAPABILITIES: readonly DappRequestType[] = [
  'sign_in',
  'sign_message',
  'sign_typed_data',
  'send_transaction',
  'request_delegation',
  'sign_out',
];

/** Which answer types complete each request type (besides `rejected`). */
export const ANSWERS: Record<DappRequestType, readonly WalletMessageType[]> = {
  sign_in: ['authenticated'],
  sign_message: ['message_signed'],
  sign_typed_data: ['typed_data_signed'],
  send_transaction: ['transaction_sent'],
  request_delegation: ['delegation_approved'],
  sign_out: ['signed_out'],
  send_privately: ['private_sent', 'transaction_sent'],
};

const REASONS: readonly RejectReason[] = ['user_rejected', 'policy_rejected', 'not_signed_in', 'network_error', 'busy'];

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === 'string';
const optStr = (v: unknown) => v === undefined || v === null || typeof v === 'string';
const ADDRESS = /^0x[0-9a-fA-F]{40}$/;
const HEX = /^0x[0-9a-fA-F]*$/;

/** Upper bound on any string field, so a hostile frame can't make us hold megabytes. */
const MAX_FIELD = 64 * 1024;
const short = (v: unknown): v is string => isStr(v) && v.length <= MAX_FIELD;

/**
 * Validates an incoming message. Anything that is not a well-formed Lowki v1
 * wallet message returns `null` and is ignored, including unknown types (so a
 * newer wallet can add messages without breaking older SDKs).
 */
export function parseWalletMessage(data: unknown): WalletToDapp | null {
  if (!isObj(data) || data.lowki !== PROTOCOL_VERSION || !isStr(data.type)) return null;
  const d = data;
  switch (d.type) {
    case 'ready':
      if (!short(d.app_id)) return null;
      if (d.capabilities !== undefined && !(Array.isArray(d.capabilities) && d.capabilities.every(short))) return null;
      return { type: 'ready', app_id: d.app_id, ...(Array.isArray(d.capabilities) ? { capabilities: d.capabilities as string[] } : {}) };
    case 'authenticated': {
      if (!optStr(d.id) || !isStr(d.address) || !ADDRESS.test(d.address) || !short(d.user_id) || !d.user_id) return null;
      if (!short(d.access_token) || !optStr(d.email) || !optStr(d.created_at)) return null;
      if (d.is_new_user !== undefined && typeof d.is_new_user !== 'boolean') return null;
      const m: WalletToDapp = {
        type: 'authenticated',
        id: (d.id as string | null | undefined) ?? null,
        address: d.address,
        user_id: d.user_id,
        access_token: d.access_token,
      };
      if (isStr(d.email)) m.email = d.email;
      if (isStr(d.created_at)) m.created_at = d.created_at;
      if (typeof d.is_new_user === 'boolean') m.is_new_user = d.is_new_user;
      return m;
    }
    case 'message_signed':
    case 'typed_data_signed':
      if (!short(d.id) || !isStr(d.signature) || !HEX.test(d.signature)) return null;
      return { type: d.type, id: d.id, signature: d.signature };
    case 'transaction_sent':
      if (!short(d.id) || !isStr(d.hash) || !HEX.test(d.hash) || typeof d.sponsored !== 'boolean') return null;
      return { type: 'transaction_sent', id: d.id, hash: d.hash, sponsored: d.sponsored };
    case 'private_sent':
      if (!short(d.id) || !isStr(d.hash) || !HEX.test(d.hash)) return null;
      return { type: 'private_sent', id: d.id, hash: d.hash };
    case 'delegation_approved':
      if (!short(d.id) || !short(d.delegation_id) || !short(d.expires) || !short(d.attestation_build)) return null;
      return { type: 'delegation_approved', id: d.id, delegation_id: d.delegation_id, expires: d.expires, attestation_build: d.attestation_build };
    case 'delegation_revoked':
      if (!short(d.delegation_id)) return null;
      return { type: 'delegation_revoked', delegation_id: d.delegation_id };
    case 'rejected':
      if (!short(d.id) || !isStr(d.reason) || !short(d.message)) return null;
      // An unknown reason from a newer wallet is still a rejection.
      return {
        type: 'rejected',
        id: d.id,
        reason: (REASONS as readonly string[]).includes(d.reason) ? (d.reason as RejectReason) : 'network_error',
        message: d.message,
      };
    case 'signed_out':
      if (!optStr(d.id)) return null;
      return { type: 'signed_out', id: (d.id as string | null | undefined) ?? null };
    default:
      return null;
  }
}

/** Wraps a request in the protocol envelope. */
export function envelope(msg: DappToWallet): { lowki: 1 } & DappToWallet {
  return { lowki: PROTOCOL_VERSION, ...msg };
}

/** The id an answer refers to, if any. */
export function answerId(m: WalletToDapp): string | null {
  return 'id' in m && typeof m.id === 'string' ? m.id : null;
}
