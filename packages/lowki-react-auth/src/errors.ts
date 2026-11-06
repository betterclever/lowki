import type { RejectReason } from './protocol';

/**
 * Privy's error codes (same string values as `PrivyErrorCode` in
 * `@privy-io/react-auth`), limited to the ones Lowki can produce plus the
 * common ones apps compare against.
 */
export const PrivyErrorCode = {
  MISSING_OR_INVALID_PRIVY_APP_ID: 'missing_or_invalid_privy_app_id',
  MISSING_OR_INVALID_TOKEN: 'missing_or_invalid_token',
  INVALID_DATA: 'invalid_data',
  UNKNOWN_AUTH_ERROR: 'unknown_auth_error',
  USER_EXITED_AUTH_FLOW: 'exited_auth_flow',
  USER_EXITED_LINK_FLOW: 'exited_link_flow',
  MUST_BE_AUTHENTICATED: 'must_be_authenticated',
  CLIENT_REQUEST_TIMEOUT: 'client_request_timeout',
  EMBEDDED_WALLET_NOT_FOUND: 'embedded_wallet_not_found',
  UNKNOWN_EMBEDDED_WALLET_ERROR: 'unknown_embedded_wallet_error',
  TRANSACTION_FAILURE: 'transaction_failure',
  UNSUPPORTED_CHAIN_ID: 'unsupported_chain_id',
  NOT_SUPPORTED: 'not_supported',
  INVALID_MESSAGE: 'invalid_message',
  UNABLE_TO_SIGN: 'unable_to_sign',
  TOO_MANY_REQUESTS: 'too_many_requests',
  INSUFFICIENT_BALANCE: 'insufficient_balance',
  UNSUPPORTED_WALLET_TYPE: 'unsupported_wallet_type',
} as const;
export type PrivyErrorCode = (typeof PrivyErrorCode)[keyof typeof PrivyErrorCode];

/** EIP-1193 / JSON-RPC numeric codes. */
export const RpcErrorCode = {
  USER_REJECTED: 4001,
  UNAUTHORIZED: 4100,
  UNSUPPORTED_METHOD: 4200,
  DISCONNECTED: 4900,
  CHAIN_DISCONNECTED: 4901,
  UNRECOGNIZED_CHAIN: 4902,
  INVALID_PARAMS: -32602,
  INTERNAL: -32603,
  RESOURCE_UNAVAILABLE: -32002,
} as const;

/**
 * Every error the SDK throws. `code` is the Privy code (what Privy's
 * `onError` callbacks receive); `rpcCode` is the EIP-1193 code the provider
 * reports; `reason` is the wallet's own reject reason when there is one.
 */
export class LowkiError extends Error {
  override name = 'LowkiError';
  readonly code: PrivyErrorCode;
  readonly rpcCode: number;
  readonly reason?: RejectReason | 'timeout' | 'origin' | 'closed' | 'not_supported' | 'not_ready';
  constructor(
    code: PrivyErrorCode,
    message: string,
    opts: { rpcCode?: number; reason?: LowkiError['reason']; cause?: unknown } = {},
  ) {
    super(message, opts.cause === undefined ? undefined : { cause: opts.cause });
    this.code = code;
    this.rpcCode = opts.rpcCode ?? RpcErrorCode.INTERNAL;
    if (opts.reason) this.reason = opts.reason;
  }
}

/** EIP-1193 `ProviderRpcError`: what viem/wagmi/ethers look at. */
export class ProviderRpcError extends Error {
  override name = 'ProviderRpcError';
  readonly code: number;
  readonly data?: unknown;
  constructor(code: number, message: string, data?: unknown) {
    super(message);
    this.code = code;
    if (data !== undefined) this.data = data;
  }
}

export function toProviderError(e: unknown): ProviderRpcError {
  if (e instanceof ProviderRpcError) return e;
  if (e instanceof LowkiError) return new ProviderRpcError(e.rpcCode, e.message, { privyCode: e.code, reason: e.reason });
  return new ProviderRpcError(RpcErrorCode.INTERNAL, e instanceof Error ? e.message : String(e));
}

/** Maps a wallet `rejected` answer to a typed error. */
export function fromReject(kind: 'login' | 'sign' | 'transaction' | 'other', reason: RejectReason, message: string): LowkiError {
  switch (reason) {
    case 'user_rejected':
      return new LowkiError(kind === 'login' ? PrivyErrorCode.USER_EXITED_AUTH_FLOW : PrivyErrorCode.UNABLE_TO_SIGN, message || 'User rejected the request', {
        rpcCode: RpcErrorCode.USER_REJECTED,
        reason,
      });
    case 'not_signed_in':
      return new LowkiError(PrivyErrorCode.MUST_BE_AUTHENTICATED, message || 'Sign in first', { rpcCode: RpcErrorCode.UNAUTHORIZED, reason });
    case 'policy_rejected':
      return new LowkiError(PrivyErrorCode.INVALID_DATA, message || 'Lowki refused the request', { rpcCode: RpcErrorCode.INVALID_PARAMS, reason });
    case 'busy':
      return new LowkiError(PrivyErrorCode.TOO_MANY_REQUESTS, message || 'Another request is open in the wallet', {
        rpcCode: RpcErrorCode.RESOURCE_UNAVAILABLE,
        reason,
      });
    case 'network_error':
    default:
      return new LowkiError(kind === 'transaction' ? PrivyErrorCode.TRANSACTION_FAILURE : PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, message || 'Network error', {
        rpcCode: RpcErrorCode.INTERNAL,
        reason,
      });
  }
}
