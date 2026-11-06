//! Sans-IO ERC-3668 (CCIP-Read) client, matching viem 2.57.16 `utils/ccip.ts`
//! (`offchainLookup`, `ccipRequest`) and `utils/ens/localBatchGatewayRequest.ts`.
//!
//! The crate never does IO. Either drive a [`CcipReadSession`] step by step (works with async
//! `fetch` in wasm), or implement [`Transport`] and call [`ccip_read`].
//!
//! Behavior copied from viem:
//! * at most [`MAX_LOOKUP_DEPTH`] (4) `OffchainLookup`s per read: the revert of the first call
//!   and up to 3 nested callback reverts; the 5th fails;
//! * `OffchainLookup.sender` must equal the called contract;
//! * if `urls` contains `x-batch-gateway:true`, the call data is an `IBatchGateway.query`
//!   handled locally (ENSIP-21), with nested local batches up to depth 4;
//! * otherwise the URLs are tried in order; a non-2xx status, a transport failure, an invalid
//!   URL scheme or a malformed body moves on to the next URL, and the last error wins;
//! * one budget of [`MAX_QUERIES`] gateway requests per read: each URL handed to a query counts
//!   once, a query without URLs counts once, and URLs beyond the budget are dropped;
//! * the callback is `callbackFunction || abi.encode(bytes response, bytes extraData)` sent to
//!   the original contract.

use alloc::string::String;
use alloc::vec::Vec;

use crate::abi::{self, Token};
use crate::{Address, Error, hex};

/// `OffchainLookup(address,string[],bytes,bytes4,bytes)`
pub const OFFCHAIN_LOOKUP_SIG: &str = "OffchainLookup(address,string[],bytes,bytes4,bytes)";
/// `0x556f1830`
pub const OFFCHAIN_LOOKUP_SELECTOR: [u8; 4] = [0x55, 0x6f, 0x18, 0x30];
/// viem's `localBatchGatewayUrl`.
pub const LOCAL_BATCH_GATEWAY_URL: &str = "x-batch-gateway:true";
/// `IBatchGateway.query((address,string[],bytes)[])`
pub const BATCH_QUERY_SIG: &str = "query((address,string[],bytes)[])";
/// viem `ccipReadLimit.depth`.
pub const MAX_LOOKUP_DEPTH: usize = 4;
/// viem `ccipReadLimit.queries`.
pub const MAX_QUERIES: usize = 256;
/// viem `maxCcipResponseBytes` (10 MiB).
pub const MAX_RESPONSE_BYTES: usize = 10_485_760;

/// The arguments of an `OffchainLookup` revert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OffchainLookup {
    pub sender: Address,
    pub urls: Vec<String>,
    pub call_data: Vec<u8>,
    pub callback_function: [u8; 4],
    pub extra_data: Vec<u8>,
}

impl OffchainLookup {
    /// Decodes revert data (selector `0x556f1830` included).
    pub fn decode(revert_data: &[u8]) -> Result<Self, Error> {
        let r = abi::expect_selector(revert_data, OFFCHAIN_LOOKUP_SELECTOR)?;
        Ok(OffchainLookup {
            sender: r.address(0)?,
            urls: r.string_array(1)?,
            call_data: r.bytes(2)?.to_vec(),
            callback_function: r.bytes4(3)?,
            extra_data: r.bytes(4)?.to_vec(),
        })
    }

    /// Encodes the revert data (for gateways, mocks and tests).
    pub fn encode(&self) -> Vec<u8> {
        abi::encode_call(
            OFFCHAIN_LOOKUP_SELECTOR,
            &[
                Token::address(&self.sender),
                Token::string_array(&self.urls),
                Token::bytes(&self.call_data),
                Token::bytes4(&self.callback_function),
                Token::bytes(&self.extra_data),
            ],
        )
    }

    /// The callback calldata for a gateway `response`.
    pub fn callback_calldata(&self, response: &[u8]) -> Vec<u8> {
        callback_calldata(self.callback_function, response, &self.extra_data)
    }
}

/// `callbackFunction || abi.encode(bytes response, bytes extraData)`.
pub fn callback_calldata(
    callback_function: [u8; 4],
    response: &[u8],
    extra_data: &[u8],
) -> Vec<u8> {
    abi::encode_call(
        callback_function,
        &[Token::bytes(response), Token::bytes(extra_data)],
    )
}

/// Is this revert data an `OffchainLookup`?
pub fn is_offchain_lookup(revert_data: &[u8]) -> bool {
    revert_data.starts_with(&OFFCHAIN_LOOKUP_SELECTOR)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        }
    }
}

/// An HTTP request to send. A POST carries `Content-Type: application/json` (see
/// [`HttpRequest::content_type`]); a GET has no headers and no body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub body: Option<String>,
}

impl HttpRequest {
    /// The `Content-Type` header viem sets (`application/json` for POST only).
    pub fn content_type(&self) -> Option<&'static str> {
        match self.method {
            HttpMethod::Post => Some("application/json"),
            HttpMethod::Get => None,
        }
    }
}

/// viem `ccipRequest` request construction for one URL template:
/// the first `{sender}` becomes the lowercase `0x` address, then the first `{data}` the `0x`
/// hex call data (JS `String.replace` with a string pattern replaces the first match only).
/// GET if the template contains `{data}`, else POST with body
/// `{"data":"0x..","sender":"0x<EIP-55 checksummed>"}` (viem passes the checksummed address
/// decoded from the revert).
pub fn ccip_request(url_template: &str, sender: &Address, data: &[u8]) -> HttpRequest {
    let data_hex = hex::encode(data);
    let url = url_template
        .replacen("{sender}", &hex::encode(sender), 1)
        .replacen("{data}", &data_hex, 1);
    if url_template.contains("{data}") {
        HttpRequest {
            method: HttpMethod::Get,
            url,
            body: None,
        }
    } else {
        let checksummed = crate::to_checksum_address(sender);
        let mut body = String::with_capacity(data_hex.len() + 64);
        body.push_str("{\"data\":\"");
        body.push_str(&data_hex);
        body.push_str("\",\"sender\":\"");
        body.push_str(&checksummed);
        body.push_str("\"}");
        HttpRequest {
            method: HttpMethod::Post,
            url,
            body: Some(body),
        }
    }
}

/// viem's `assertCcipUrl`, approximated: the URL (after trimming leading/trailing C0 controls
/// and spaces, as the WHATWG URL parser does) must have scheme `http` or `https`.
pub fn is_allowed_url(url: &str) -> bool {
    let t = url.trim_matches(|c: char| c <= ' ');
    let Some((scheme, _)) = t.split_once(':') else {
        return false;
    };
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

/// An HTTP response as seen by the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// The `Content-Type` header, if any.
    pub content_type: Option<String>,
    pub body: String,
}

/// A request that produced no response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpFailure {
    /// Network / fetch failure (viem `HttpRequestError` without a status).
    Network,
    /// viem `TimeoutError`.
    Timeout,
    /// viem `ResponseBodyTooLargeError`.
    BodyTooLarge,
}

/// Why one gateway query failed (the last error over its URLs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayError {
    /// Non-2xx status (`HttpRequestError` with `status`).
    Http {
        status: u16,
    },
    /// `HttpRequestError` without a status: network failure, invalid URL, unparsable JSON,
    /// query budget or batch depth exceeded.
    Request,
    Timeout,
    BodyTooLarge,
    /// The body was not `0x` hex (`OffchainLookupResponseMalformedError`).
    Malformed,
    /// No URL was tried (`new Error('An unknown error occurred.')`).
    NoUrls,
    /// A nested batch query whose data is shorter than 4 bytes or does not start with the
    /// `query` selector (viem `AbiFunctionSignatureNotFoundError`; `len` = selector bytes
    /// present, 0..=4).
    UnknownBatchSelector {
        selector: [u8; 4],
        len: u8,
    },
    /// Any other undecodable nested batch query (viem fails it with an ABI decoding error
    /// whose message this crate does not reproduce; see the README).
    InvalidBatch,
}

impl From<HttpFailure> for GatewayError {
    fn from(f: HttpFailure) -> Self {
        match f {
            HttpFailure::Network => GatewayError::Request,
            HttpFailure::Timeout => GatewayError::Timeout,
            HttpFailure::BodyTooLarge => GatewayError::BodyTooLarge,
        }
    }
}

impl GatewayError {
    /// viem's `shortMessage` of the corresponding error.
    pub fn short_message(&self) -> String {
        if let GatewayError::UnknownBatchSelector { selector, len } = self {
            let sig = hex::encode(&selector[..*len as usize]);
            return alloc::format!(
                "Encoded function signature \"{sig}\" not found on ABI.\nMake sure you are using the correct ABI and that the function exists on it.\nYou can look up the signature here: https://4byte.sourcify.dev/?q={sig}."
            );
        }
        String::from(match self {
            GatewayError::Http { .. } | GatewayError::Request => "HTTP request failed.",
            GatewayError::Timeout => "The request took too long to respond.",
            GatewayError::BodyTooLarge => "HTTP response body exceeded the size limit.",
            GatewayError::Malformed => {
                "Offchain gateway response is malformed. Response data must be a hex value."
            }
            GatewayError::NoUrls => "An unknown error occurred.",
            GatewayError::InvalidBatch => "Invalid batch gateway request.",
            GatewayError::UnknownBatchSelector { .. } => unreachable!(),
        })
    }

    /// The failure bytes of a local batch gateway response (viem `encodeError`):
    /// `HttpError(uint16,string)` for an HTTP status (non-zero), else `Error(string)`.
    pub fn encode_batch_failure(&self) -> Vec<u8> {
        match self {
            GatewayError::Http { status } if *status != 0 => abi::encode_sig(
                crate::universal_resolver::HTTP_ERROR_SIG,
                &[
                    Token::uint(*status as u64),
                    Token::string(&self.short_message()),
                ],
            ),
            _ => abi::encode_sig(
                crate::universal_resolver::ERROR_STRING_SIG,
                &[Token::string(&self.short_message())],
            ),
        }
    }
}

enum JsonData {
    Str(String),
    Other,
}

/// Parses a gateway response like viem's `ccipRequest`: JSON `.data` when `Content-Type`
/// starts with `application/json` (case-sensitive), else the raw body; non-2xx is
/// [`GatewayError::Http`]; anything that is not `0x` hex (or has odd length) is
/// [`GatewayError::Malformed`].
pub fn parse_gateway_response(resp: &HttpResponse) -> Result<Vec<u8>, GatewayError> {
    if resp.body.len() > MAX_RESPONSE_BYTES {
        return Err(GatewayError::BodyTooLarge);
    }
    let json = resp
        .content_type
        .as_deref()
        .is_some_and(|c| c.starts_with("application/json"));
    let result = if json {
        let v: serde_json::Value =
            serde_json::from_str(&resp.body).map_err(|_| GatewayError::Request)?;
        match v {
            // `JSON.parse("null").data` throws a TypeError, caught as an HttpRequestError.
            serde_json::Value::Null => return Err(GatewayError::Request),
            serde_json::Value::Object(m) => match m.get("data") {
                Some(serde_json::Value::String(s)) => JsonData::Str(s.clone()),
                _ => JsonData::Other,
            },
            _ => JsonData::Other,
        }
    } else {
        JsonData::Str(resp.body.clone())
    };
    if !(200..=299).contains(&resp.status) {
        return Err(GatewayError::Http {
            status: resp.status,
        });
    }
    match result {
        JsonData::Str(s) if hex::is_hex(&s) => hex::decode(&s).map_err(|_| GatewayError::Malformed),
        _ => Err(GatewayError::Malformed),
    }
}

/// One `IBatchGateway.Request`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchRequest {
    pub sender: Address,
    pub urls: Vec<String>,
    pub data: Vec<u8>,
}

/// `IBatchGateway.query(Request[])` calldata.
pub fn encode_batch_query(requests: &[BatchRequest]) -> Vec<u8> {
    let items = requests
        .iter()
        .map(|q| {
            Token::Tuple(alloc::vec![
                Token::address(&q.sender),
                Token::string_array(&q.urls),
                Token::bytes(&q.data),
            ])
        })
        .collect();
    abi::encode_sig(BATCH_QUERY_SIG, &[Token::Array(items)])
}

/// Decodes `IBatchGateway.query(Request[])` calldata.
pub fn decode_batch_query(call_data: &[u8]) -> Result<Vec<BatchRequest>, Error> {
    let r = abi::expect_selector(call_data, abi::selector(BATCH_QUERY_SIG))?;
    let (n, el) = r.array(0, 1)?;
    (0..n)
        .map(|i| {
            let t = el.frame(i)?;
            Ok(BatchRequest {
                sender: t.address(0)?,
                urls: t.string_array(1)?,
                data: t.bytes(2)?.to_vec(),
            })
        })
        .collect()
}

/// `abi.encode(bool[] failures, bytes[] responses)` (the `query` return).
pub fn encode_batch_response<B: AsRef<[u8]>>(failures: &[bool], responses: &[B]) -> Vec<u8> {
    abi::encode(&[
        Token::Array(failures.iter().map(|&b| Token::bool(b)).collect()),
        Token::bytes_array(responses),
    ])
}

/// Decodes `(bool[] failures, bytes[] responses)`.
pub fn decode_batch_response(data: &[u8]) -> Result<(Vec<bool>, Vec<Vec<u8>>), Error> {
    let r = abi::Reader::new(data);
    Ok((r.bool_array(0)?, r.bytes_array(1)?))
}

/// Result of an `eth_call`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallOutcome {
    Success(Vec<u8>),
    /// The call reverted with this data.
    Revert(Vec<u8>),
}

/// Why a CCIP-Read failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CcipError {
    /// The call reverted with something other than `OffchainLookup`.
    CallReverted(Vec<u8>),
    /// `OffchainLookup` revert data could not be decoded.
    InvalidOffchainLookup,
    /// `OffchainLookup.sender` is not the called contract.
    SenderMismatch { sender: Address, to: Address },
    /// More than [`MAX_LOOKUP_DEPTH`] lookups.
    DepthExceeded,
    /// The query budget ([`MAX_QUERIES`]) was used up.
    QueryBudgetExceeded,
    /// Every gateway URL failed; the last error.
    Gateway(GatewayError),
    /// The call data of a local batch lookup is not an `IBatchGateway.query` call.
    InvalidBatchCall,
    /// The session was fed the wrong kind of input, or fed after it finished.
    UnexpectedInput,
}

/// What the caller must do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Perform `eth_call(to, data)` and feed the outcome to [`CcipReadSession::on_eth_call`].
    EthCall {
        to: Address,
        data: Vec<u8>,
    },
    /// Send the request and feed the result to [`CcipReadSession::on_http`].
    Http(HttpRequest),
    /// The final return data.
    Done(Vec<u8>),
    Failed(CcipError),
}

#[derive(Clone, Debug)]
struct Cursor {
    sender: Address,
    data: Vec<u8>,
    urls: Vec<String>,
    idx: usize,
    last: GatewayError,
}

enum CursorStep {
    Request(HttpRequest),
    Exhausted(GatewayError),
}

impl Cursor {
    fn new(sender: Address, data: Vec<u8>, urls: Vec<String>) -> Self {
        Cursor {
            sender,
            data,
            urls,
            idx: 0,
            last: GatewayError::NoUrls,
        }
    }

    fn next(&mut self) -> CursorStep {
        while self.idx < self.urls.len() {
            let req = ccip_request(&self.urls[self.idx], &self.sender, &self.data);
            if is_allowed_url(&req.url) {
                return CursorStep::Request(req);
            }
            self.last = GatewayError::Request;
            self.idx += 1;
        }
        CursorStep::Exhausted(self.last)
    }
}

#[derive(Clone, Debug)]
struct Frame {
    queries: Vec<BatchRequest>,
    failures: Vec<bool>,
    responses: Vec<Vec<u8>>,
}

impl Frame {
    fn new(queries: Vec<BatchRequest>) -> Self {
        Frame {
            queries,
            failures: Vec::new(),
            responses: Vec::new(),
        }
    }
    fn push(&mut self, r: Result<Vec<u8>, GatewayError>) {
        match r {
            Ok(v) => {
                self.failures.push(false);
                self.responses.push(v);
            }
            Err(e) => {
                self.failures.push(true);
                self.responses.push(e.encode_batch_failure());
            }
        }
    }
}

enum State {
    AwaitCall,
    Direct {
        callback: [u8; 4],
        extra: Vec<u8>,
        cursor: Cursor,
    },
    Batch {
        callback: [u8; 4],
        extra: Vec<u8>,
        stack: Vec<Frame>,
        cursor: Option<Cursor>,
    },
    Finished,
}

/// Step-wise CCIP-Read driver (see the module docs).
pub struct CcipReadSession {
    to: Address,
    lookups: usize,
    budget: usize,
    state: State,
}

impl CcipReadSession {
    /// Starts a read of `data` on `to`; the first step is always that `eth_call`.
    pub fn start(to: Address, data: Vec<u8>) -> (Self, Step) {
        let s = CcipReadSession {
            to,
            lookups: 0,
            budget: MAX_QUERIES,
            state: State::AwaitCall,
        };
        (s, Step::EthCall { to, data })
    }

    /// The contract being read.
    pub fn to(&self) -> Address {
        self.to
    }

    /// Gateway requests still allowed.
    pub fn remaining_queries(&self) -> usize {
        self.budget
    }

    fn fail(&mut self, e: CcipError) -> Step {
        self.state = State::Finished;
        Step::Failed(e)
    }

    /// Feeds the outcome of the last [`Step::EthCall`].
    pub fn on_eth_call(&mut self, outcome: CallOutcome) -> Step {
        if !matches!(self.state, State::AwaitCall) {
            return self.fail(CcipError::UnexpectedInput);
        }
        match outcome {
            CallOutcome::Success(d) => {
                self.state = State::Finished;
                Step::Done(d)
            }
            CallOutcome::Revert(d) if is_offchain_lookup(&d) => self.lookup(&d),
            CallOutcome::Revert(d) => self.fail(CcipError::CallReverted(d)),
        }
    }

    fn lookup(&mut self, revert: &[u8]) -> Step {
        let Ok(l) = OffchainLookup::decode(revert) else {
            return self.fail(CcipError::InvalidOffchainLookup);
        };
        if self.lookups >= MAX_LOOKUP_DEPTH {
            return self.fail(CcipError::DepthExceeded);
        }
        self.lookups += 1;
        if l.sender != self.to {
            return self.fail(CcipError::SenderMismatch {
                sender: l.sender,
                to: self.to,
            });
        }
        if l.urls.iter().any(|u| u == LOCAL_BATCH_GATEWAY_URL) {
            let Ok(queries) = decode_batch_query(&l.call_data) else {
                return self.fail(CcipError::InvalidBatchCall);
            };
            self.state = State::Batch {
                callback: l.callback_function,
                extra: l.extra_data,
                stack: alloc::vec![Frame::new(queries)],
                cursor: None,
            };
            return self.drive_batch();
        }
        if self.budget == 0 {
            return self.fail(CcipError::QueryBudgetExceeded);
        }
        let n = l.urls.len().min(self.budget);
        self.budget -= n.max(1);
        let mut urls = l.urls;
        urls.truncate(n);
        let mut cursor = Cursor::new(l.sender, l.call_data, urls);
        match cursor.next() {
            CursorStep::Request(r) => {
                self.state = State::Direct {
                    callback: l.callback_function,
                    extra: l.extra_data,
                    cursor,
                };
                Step::Http(r)
            }
            CursorStep::Exhausted(e) => self.fail(CcipError::Gateway(e)),
        }
    }

    fn callback(&mut self, callback: [u8; 4], response: &[u8], extra: &[u8]) -> Step {
        self.state = State::AwaitCall;
        Step::EthCall {
            to: self.to,
            data: callback_calldata(callback, response, extra),
        }
    }

    /// Feeds the result of the last [`Step::Http`].
    pub fn on_http(&mut self, result: Result<HttpResponse, HttpFailure>) -> Step {
        let parsed = match result {
            Ok(r) => parse_gateway_response(&r),
            Err(f) => Err(f.into()),
        };
        match &mut self.state {
            State::Direct { cursor, .. } => match parsed {
                Ok(v) => {
                    let State::Direct {
                        callback, extra, ..
                    } = core::mem::replace(&mut self.state, State::Finished)
                    else {
                        unreachable!()
                    };
                    self.callback(callback, &v, &extra)
                }
                Err(e) => {
                    cursor.last = e;
                    cursor.idx += 1;
                    match cursor.next() {
                        CursorStep::Request(r) => Step::Http(r),
                        CursorStep::Exhausted(e) => self.fail(CcipError::Gateway(e)),
                    }
                }
            },
            State::Batch {
                stack,
                cursor: cur @ Some(_),
                ..
            } => {
                let cursor = cur.as_mut().expect("some");
                match parsed {
                    Ok(v) => {
                        *cur = None;
                        stack.last_mut().expect("frame").push(Ok(v));
                    }
                    Err(e) => {
                        cursor.last = e;
                        cursor.idx += 1;
                        match cursor.next() {
                            CursorStep::Request(r) => return Step::Http(r),
                            CursorStep::Exhausted(e) => {
                                *cur = None;
                                stack.last_mut().expect("frame").push(Err(e));
                            }
                        }
                    }
                }
                self.drive_batch()
            }
            _ => self.fail(CcipError::UnexpectedInput),
        }
    }

    /// Advances the local batch gateway until it needs an HTTP response or is complete.
    fn drive_batch(&mut self) -> Step {
        loop {
            let State::Batch { stack, cursor, .. } = &mut self.state else {
                unreachable!("drive_batch outside a batch")
            };
            let depth = stack.len() - 1;
            let top = stack.last_mut().expect("frame");
            let i = top.failures.len();
            if i == top.queries.len() {
                let resp = encode_batch_response(&top.failures, &top.responses);
                stack.pop();
                match stack.last_mut() {
                    Some(parent) => {
                        parent.push(Ok(resp));
                        continue;
                    }
                    None => {
                        let State::Batch {
                            callback, extra, ..
                        } = core::mem::replace(&mut self.state, State::Finished)
                        else {
                            unreachable!()
                        };
                        return self.callback(callback, &resp, &extra);
                    }
                }
            }
            let q = &top.queries[i];
            if q.urls.iter().any(|u| u == LOCAL_BATCH_GATEWAY_URL) {
                if depth + 1 >= MAX_LOOKUP_DEPTH {
                    top.push(Err(GatewayError::Request));
                    continue;
                }
                match decode_batch_query(&q.data) {
                    Ok(qs) => stack.push(Frame::new(qs)),
                    Err(_) => top.push(Err(batch_decode_error(&q.data))),
                }
                continue;
            }
            if self.budget == 0 {
                top.push(Err(GatewayError::Request));
                continue;
            }
            let n = q.urls.len().min(self.budget);
            self.budget -= n.max(1);
            let mut c = Cursor::new(q.sender, q.data.clone(), q.urls[..n].to_vec());
            match c.next() {
                CursorStep::Request(r) => {
                    *cursor = Some(c);
                    return Step::Http(r);
                }
                CursorStep::Exhausted(e) => top.push(Err(e)),
            }
        }
    }
}

fn batch_decode_error(data: &[u8]) -> GatewayError {
    let n = data.len().min(4);
    let mut selector = [0u8; 4];
    selector[..n].copy_from_slice(&data[..n]);
    if n < 4 || selector != abi::selector(BATCH_QUERY_SIG) {
        GatewayError::UnknownBatchSelector {
            selector,
            len: n as u8,
        }
    } else {
        GatewayError::InvalidBatch
    }
}

/// IO for [`ccip_read`].
pub trait Transport {
    /// `eth_call(to, data)`. Return `Ok(CallOutcome::Revert(data))` for a revert with data;
    /// `Err` aborts the read with that error.
    fn eth_call(&mut self, to: &Address, data: &[u8]) -> Result<CallOutcome, Error>;
    /// Sends one gateway request.
    fn http(&mut self, req: &HttpRequest) -> Result<HttpResponse, HttpFailure>;
}

/// Synchronous CCIP-Read driver over a [`Transport`].
pub fn ccip_read<T: Transport + ?Sized>(
    t: &mut T,
    to: Address,
    data: &[u8],
) -> Result<Vec<u8>, Error> {
    let (mut s, mut step) = CcipReadSession::start(to, data.to_vec());
    loop {
        step = match step {
            Step::EthCall { to, data } => {
                let o = t.eth_call(&to, &data)?;
                s.on_eth_call(o)
            }
            Step::Http(req) => {
                let r = t.http(&req);
                s.on_http(r)
            }
            Step::Done(d) => return Ok(d),
            Step::Failed(e) => return Err(Error::Ccip(e)),
        }
    }
}
