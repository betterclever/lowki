var __defProp = Object.defineProperty;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e) {
    throw err = [e], e;
  }
};
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// node_modules/viem/node_modules/abitype/dist/esm/regex.js
function execTyped(regex, string) {
  const match = regex.exec(string);
  return match?.groups;
}
var init_regex = __esm({
  "node_modules/viem/node_modules/abitype/dist/esm/regex.js"() {
  }
});

// node_modules/viem/node_modules/abitype/dist/esm/human-readable/formatAbiParameter.js
function formatAbiParameter(abiParameter) {
  let type = abiParameter.type;
  if (tupleRegex.test(abiParameter.type) && "components" in abiParameter) {
    type = "(";
    const length = abiParameter.components.length;
    for (let i = 0; i < length; i++) {
      const component = abiParameter.components[i];
      type += formatAbiParameter(component);
      if (i < length - 1)
        type += ", ";
    }
    const result = execTyped(tupleRegex, abiParameter.type);
    type += `)${result?.array || ""}`;
    return formatAbiParameter({
      ...abiParameter,
      type
    });
  }
  if ("indexed" in abiParameter && abiParameter.indexed)
    type = `${type} indexed`;
  if (abiParameter.name)
    return `${type} ${abiParameter.name}`;
  return type;
}
var tupleRegex;
var init_formatAbiParameter = __esm({
  "node_modules/viem/node_modules/abitype/dist/esm/human-readable/formatAbiParameter.js"() {
    init_regex();
    tupleRegex = /^tuple(?<array>(\[(\d*)\])*)$/;
  }
});

// node_modules/viem/node_modules/abitype/dist/esm/human-readable/formatAbiParameters.js
function formatAbiParameters(abiParameters) {
  let params = "";
  const length = abiParameters.length;
  for (let i = 0; i < length; i++) {
    const abiParameter = abiParameters[i];
    params += formatAbiParameter(abiParameter);
    if (i !== length - 1)
      params += ", ";
  }
  return params;
}
var init_formatAbiParameters = __esm({
  "node_modules/viem/node_modules/abitype/dist/esm/human-readable/formatAbiParameters.js"() {
    init_formatAbiParameter();
  }
});

// node_modules/viem/node_modules/abitype/dist/esm/human-readable/formatAbiItem.js
function formatAbiItem(abiItem) {
  if (abiItem.type === "function")
    return `function ${abiItem.name}(${formatAbiParameters(abiItem.inputs)})${abiItem.stateMutability && abiItem.stateMutability !== "nonpayable" ? ` ${abiItem.stateMutability}` : ""}${abiItem.outputs?.length ? ` returns (${formatAbiParameters(abiItem.outputs)})` : ""}`;
  if (abiItem.type === "event")
    return `event ${abiItem.name}(${formatAbiParameters(abiItem.inputs)})`;
  if (abiItem.type === "error")
    return `error ${abiItem.name}(${formatAbiParameters(abiItem.inputs)})`;
  if (abiItem.type === "constructor")
    return `constructor(${formatAbiParameters(abiItem.inputs)})${abiItem.stateMutability === "payable" ? " payable" : ""}`;
  if (abiItem.type === "fallback")
    return `fallback() external${abiItem.stateMutability === "payable" ? " payable" : ""}`;
  return "receive() external payable";
}
var init_formatAbiItem = __esm({
  "node_modules/viem/node_modules/abitype/dist/esm/human-readable/formatAbiItem.js"() {
    init_formatAbiParameters();
  }
});

// node_modules/viem/node_modules/abitype/dist/esm/exports/index.js
var init_exports = __esm({
  "node_modules/viem/node_modules/abitype/dist/esm/exports/index.js"() {
    init_formatAbiItem();
  }
});

// node_modules/viem/_esm/utils/abi/formatAbiItem.js
function formatAbiItem2(abiItem, { includeName = false } = {}) {
  if (abiItem.type !== "function" && abiItem.type !== "event" && abiItem.type !== "error")
    throw new InvalidDefinitionTypeError(abiItem.type);
  return `${abiItem.name}(${formatAbiParams(abiItem.inputs, { includeName })})`;
}
function formatAbiParams(params, { includeName = false } = {}) {
  if (!params)
    return "";
  return params.map((param) => formatAbiParam(param, { includeName })).join(includeName ? ", " : ",");
}
function formatAbiParam(param, { includeName }) {
  if (param.type.startsWith("tuple")) {
    return `(${formatAbiParams(param.components, { includeName })})${param.type.slice("tuple".length)}`;
  }
  return param.type + (includeName && param.name ? ` ${param.name}` : "");
}
var init_formatAbiItem2 = __esm({
  "node_modules/viem/_esm/utils/abi/formatAbiItem.js"() {
    init_abi();
  }
});

// node_modules/viem/_esm/utils/data/isHex.js
function isHex(value, { strict = true } = {}) {
  if (!value)
    return false;
  if (typeof value !== "string")
    return false;
  return strict ? /^0x[0-9a-fA-F]*$/.test(value) : value.startsWith("0x");
}
var init_isHex = __esm({
  "node_modules/viem/_esm/utils/data/isHex.js"() {
  }
});

// node_modules/viem/_esm/utils/data/size.js
function size(value) {
  if (isHex(value, { strict: false }))
    return Math.ceil((value.length - 2) / 2);
  return value.length;
}
var init_size = __esm({
  "node_modules/viem/_esm/utils/data/size.js"() {
    init_isHex();
  }
});

// node_modules/viem/_esm/errors/version.js
var version;
var init_version = __esm({
  "node_modules/viem/_esm/errors/version.js"() {
    version = "2.54.1";
  }
});

// node_modules/viem/_esm/errors/base.js
function walk(err, fn) {
  if (fn?.(err))
    return err;
  if (err && typeof err === "object" && "cause" in err && err.cause !== void 0)
    return walk(err.cause, fn);
  return fn ? null : err;
}
var errorConfig, BaseError;
var init_base = __esm({
  "node_modules/viem/_esm/errors/base.js"() {
    init_version();
    errorConfig = {
      getDocsUrl: ({ docsBaseUrl, docsPath: docsPath3 = "", docsSlug }) => docsPath3 ? `${docsBaseUrl ?? "https://viem.sh"}${docsPath3}${docsSlug ? `#${docsSlug}` : ""}` : void 0,
      version: `viem@${version}`
    };
    BaseError = class _BaseError extends Error {
      constructor(shortMessage, args = {}) {
        const details = (() => {
          if (args.cause instanceof _BaseError)
            return args.cause.details;
          if (args.cause?.message)
            return args.cause.message;
          return args.details;
        })();
        const docsPath3 = (() => {
          if (args.cause instanceof _BaseError)
            return args.cause.docsPath || args.docsPath;
          return args.docsPath;
        })();
        const docsUrl = errorConfig.getDocsUrl?.({ ...args, docsPath: docsPath3 });
        const message = [
          shortMessage || "An error occurred.",
          "",
          ...args.metaMessages ? [...args.metaMessages, ""] : [],
          ...docsUrl ? [`Docs: ${docsUrl}`] : [],
          ...details ? [`Details: ${details}`] : [],
          ...errorConfig.version ? [`Version: ${errorConfig.version}`] : []
        ].join("\n");
        super(message, args.cause ? { cause: args.cause } : void 0);
        Object.defineProperty(this, "details", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        Object.defineProperty(this, "docsPath", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        Object.defineProperty(this, "metaMessages", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        Object.defineProperty(this, "shortMessage", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        Object.defineProperty(this, "version", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        Object.defineProperty(this, "name", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: "BaseError"
        });
        this.details = details;
        this.docsPath = docsPath3;
        this.metaMessages = args.metaMessages;
        this.name = args.name ?? this.name;
        this.shortMessage = shortMessage;
        this.version = version;
      }
      walk(fn) {
        return walk(this, fn);
      }
    };
  }
});

// node_modules/viem/_esm/errors/abi.js
var AbiDecodingDataSizeTooSmallError, AbiDecodingZeroDataError, AbiEncodingArrayLengthMismatchError, AbiEncodingBytesSizeMismatchError, AbiEncodingLengthMismatchError, AbiFunctionNotFoundError, AbiFunctionOutputsNotFoundError, AbiItemAmbiguityError, InvalidAbiEncodingTypeError, InvalidAbiDecodingTypeError, InvalidArrayError, InvalidDefinitionTypeError;
var init_abi = __esm({
  "node_modules/viem/_esm/errors/abi.js"() {
    init_formatAbiItem2();
    init_size();
    init_base();
    AbiDecodingDataSizeTooSmallError = class extends BaseError {
      constructor({ data, params, size: size3 }) {
        super([`Data size of ${size3} bytes is too small for given parameters.`].join("\n"), {
          metaMessages: [
            `Params: (${formatAbiParams(params, { includeName: true })})`,
            `Data:   ${data} (${size3} bytes)`
          ],
          name: "AbiDecodingDataSizeTooSmallError"
        });
        Object.defineProperty(this, "data", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        Object.defineProperty(this, "params", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        Object.defineProperty(this, "size", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        this.data = data;
        this.params = params;
        this.size = size3;
      }
    };
    AbiDecodingZeroDataError = class extends BaseError {
      constructor({ cause } = {}) {
        super('Cannot decode zero data ("0x") with ABI parameters.', {
          name: "AbiDecodingZeroDataError",
          cause
        });
      }
    };
    AbiEncodingArrayLengthMismatchError = class extends BaseError {
      constructor({ expectedLength, givenLength, type }) {
        super([
          `ABI encoding array length mismatch for type ${type}.`,
          `Expected length: ${expectedLength}`,
          `Given length: ${givenLength}`
        ].join("\n"), { name: "AbiEncodingArrayLengthMismatchError" });
      }
    };
    AbiEncodingBytesSizeMismatchError = class extends BaseError {
      constructor({ expectedSize, value }) {
        super(`Size of bytes "${value}" (bytes${size(value)}) does not match expected size (bytes${expectedSize}).`, { name: "AbiEncodingBytesSizeMismatchError" });
      }
    };
    AbiEncodingLengthMismatchError = class extends BaseError {
      constructor({ expectedLength, givenLength }) {
        super([
          "ABI encoding params/values length mismatch.",
          `Expected length (params): ${expectedLength}`,
          `Given length (values): ${givenLength}`
        ].join("\n"), { name: "AbiEncodingLengthMismatchError" });
      }
    };
    AbiFunctionNotFoundError = class extends BaseError {
      constructor(functionName, { docsPath: docsPath3 } = {}) {
        super([
          `Function ${functionName ? `"${functionName}" ` : ""}not found on ABI.`,
          "Make sure you are using the correct ABI and that the function exists on it."
        ].join("\n"), {
          docsPath: docsPath3,
          name: "AbiFunctionNotFoundError"
        });
      }
    };
    AbiFunctionOutputsNotFoundError = class extends BaseError {
      constructor(functionName, { docsPath: docsPath3 }) {
        super([
          `Function "${functionName}" does not contain any \`outputs\` on ABI.`,
          "Cannot decode function result without knowing what the parameter types are.",
          "Make sure you are using the correct ABI and that the function exists on it."
        ].join("\n"), {
          docsPath: docsPath3,
          name: "AbiFunctionOutputsNotFoundError"
        });
      }
    };
    AbiItemAmbiguityError = class extends BaseError {
      constructor(x, y) {
        super("Found ambiguous types in overloaded ABI items.", {
          metaMessages: [
            `\`${x.type}\` in \`${formatAbiItem2(x.abiItem)}\`, and`,
            `\`${y.type}\` in \`${formatAbiItem2(y.abiItem)}\``,
            "",
            "These types encode differently and cannot be distinguished at runtime.",
            "Remove one of the ambiguous items in the ABI."
          ],
          name: "AbiItemAmbiguityError"
        });
      }
    };
    InvalidAbiEncodingTypeError = class extends BaseError {
      constructor(type, { docsPath: docsPath3 }) {
        super([
          `Type "${type}" is not a valid encoding type.`,
          "Please provide a valid ABI type."
        ].join("\n"), { docsPath: docsPath3, name: "InvalidAbiEncodingType" });
      }
    };
    InvalidAbiDecodingTypeError = class extends BaseError {
      constructor(type, { docsPath: docsPath3 }) {
        super([
          `Type "${type}" is not a valid decoding type.`,
          "Please provide a valid ABI type."
        ].join("\n"), { docsPath: docsPath3, name: "InvalidAbiDecodingType" });
      }
    };
    InvalidArrayError = class extends BaseError {
      constructor(value) {
        super([`Value "${value}" is not a valid array.`].join("\n"), {
          name: "InvalidArrayError"
        });
      }
    };
    InvalidDefinitionTypeError = class extends BaseError {
      constructor(type) {
        super([
          `"${type}" is not a valid definition type.`,
          'Valid types: "function", "event", "error"'
        ].join("\n"), { name: "InvalidDefinitionTypeError" });
      }
    };
  }
});

// node_modules/viem/_esm/errors/data.js
var SliceOffsetOutOfBoundsError, SizeExceedsPaddingSizeError;
var init_data = __esm({
  "node_modules/viem/_esm/errors/data.js"() {
    init_base();
    SliceOffsetOutOfBoundsError = class extends BaseError {
      constructor({ offset, position, size: size3 }) {
        super(`Slice ${position === "start" ? "starting" : "ending"} at offset "${offset}" is out-of-bounds (size: ${size3}).`, { name: "SliceOffsetOutOfBoundsError" });
      }
    };
    SizeExceedsPaddingSizeError = class extends BaseError {
      constructor({ size: size3, targetSize, type }) {
        super(`${type.charAt(0).toUpperCase()}${type.slice(1).toLowerCase()} size (${size3}) exceeds padding size (${targetSize}).`, { name: "SizeExceedsPaddingSizeError" });
      }
    };
  }
});

// node_modules/viem/_esm/utils/data/pad.js
function pad(hexOrBytes, { dir, size: size3 = 32 } = {}) {
  if (typeof hexOrBytes === "string")
    return padHex(hexOrBytes, { dir, size: size3 });
  return padBytes(hexOrBytes, { dir, size: size3 });
}
function padHex(hex_, { dir, size: size3 = 32 } = {}) {
  if (size3 === null)
    return hex_;
  const hex = hex_.replace("0x", "");
  if (hex.length > size3 * 2)
    throw new SizeExceedsPaddingSizeError({
      size: Math.ceil(hex.length / 2),
      targetSize: size3,
      type: "hex"
    });
  return `0x${hex[dir === "right" ? "padEnd" : "padStart"](size3 * 2, "0")}`;
}
function padBytes(bytes, { dir, size: size3 = 32 } = {}) {
  if (size3 === null)
    return bytes;
  if (bytes.length > size3)
    throw new SizeExceedsPaddingSizeError({
      size: bytes.length,
      targetSize: size3,
      type: "bytes"
    });
  const paddedBytes = new Uint8Array(size3);
  for (let i = 0; i < size3; i++) {
    const padEnd = dir === "right";
    paddedBytes[padEnd ? i : size3 - i - 1] = bytes[padEnd ? i : bytes.length - i - 1];
  }
  return paddedBytes;
}
var init_pad = __esm({
  "node_modules/viem/_esm/utils/data/pad.js"() {
    init_data();
  }
});

// node_modules/viem/_esm/errors/encoding.js
var IntegerOutOfRangeError, InvalidBytesBooleanError, SizeOverflowError;
var init_encoding = __esm({
  "node_modules/viem/_esm/errors/encoding.js"() {
    init_base();
    IntegerOutOfRangeError = class extends BaseError {
      constructor({ max, min, signed, size: size3, value }) {
        super(`Number "${value}" is not in safe ${size3 ? `${size3 * 8}-bit ${signed ? "signed" : "unsigned"} ` : ""}integer range ${max ? `(${min} to ${max})` : `(above ${min})`}`, { name: "IntegerOutOfRangeError" });
      }
    };
    InvalidBytesBooleanError = class extends BaseError {
      constructor(bytes) {
        super(`Bytes value "${bytes}" is not a valid boolean. The bytes array must contain a single byte of either a 0 or 1 value.`, {
          name: "InvalidBytesBooleanError"
        });
      }
    };
    SizeOverflowError = class extends BaseError {
      constructor({ givenSize, maxSize }) {
        super(`Size cannot exceed ${maxSize} bytes. Given size: ${givenSize} bytes.`, { name: "SizeOverflowError" });
      }
    };
  }
});

// node_modules/viem/_esm/utils/data/trim.js
function trim(hexOrBytes, { dir = "left" } = {}) {
  let data = typeof hexOrBytes === "string" ? hexOrBytes.replace("0x", "") : hexOrBytes;
  let sliceLength = 0;
  for (let i = 0; i < data.length - 1; i++) {
    if (data[dir === "left" ? i : data.length - i - 1].toString() === "0")
      sliceLength++;
    else
      break;
  }
  data = dir === "left" ? data.slice(sliceLength) : data.slice(0, data.length - sliceLength);
  if (typeof hexOrBytes === "string") {
    if (data.length === 1 && dir === "right")
      data = `${data}0`;
    return `0x${data.length % 2 === 1 ? `0${data}` : data}`;
  }
  return data;
}
var init_trim = __esm({
  "node_modules/viem/_esm/utils/data/trim.js"() {
  }
});

// node_modules/viem/_esm/utils/encoding/fromHex.js
function assertSize(hexOrBytes, { size: size3 }) {
  if (size(hexOrBytes) > size3)
    throw new SizeOverflowError({
      givenSize: size(hexOrBytes),
      maxSize: size3
    });
}
function hexToBigInt(hex, opts = {}) {
  const { signed } = opts;
  if (opts.size)
    assertSize(hex, { size: opts.size });
  const value = BigInt(hex);
  if (!signed)
    return value;
  const size3 = (hex.length - 2) / 2;
  const max = (1n << BigInt(size3) * 8n - 1n) - 1n;
  if (value <= max)
    return value;
  return value - BigInt(`0x${"f".padStart(size3 * 2, "f")}`) - 1n;
}
function hexToNumber(hex, opts = {}) {
  const value = hexToBigInt(hex, opts);
  const number = Number(value);
  if (!Number.isSafeInteger(number))
    throw new IntegerOutOfRangeError({
      max: `${Number.MAX_SAFE_INTEGER}`,
      min: `${Number.MIN_SAFE_INTEGER}`,
      signed: opts.signed,
      size: opts.size,
      value: `${value}n`
    });
  return number;
}
var init_fromHex = __esm({
  "node_modules/viem/_esm/utils/encoding/fromHex.js"() {
    init_encoding();
    init_size();
  }
});

// node_modules/viem/_esm/utils/encoding/toHex.js
function toHex(value, opts = {}) {
  if (typeof value === "number" || typeof value === "bigint")
    return numberToHex(value, opts);
  if (typeof value === "string") {
    return stringToHex(value, opts);
  }
  if (typeof value === "boolean")
    return boolToHex(value, opts);
  return bytesToHex(value, opts);
}
function boolToHex(value, opts = {}) {
  const hex = `0x${Number(value)}`;
  if (typeof opts.size === "number") {
    assertSize(hex, { size: opts.size });
    return pad(hex, { size: opts.size });
  }
  return hex;
}
function bytesToHex(value, opts = {}) {
  let string = "";
  for (let i = 0; i < value.length; i++) {
    string += hexes[value[i]];
  }
  const hex = `0x${string}`;
  if (typeof opts.size === "number") {
    assertSize(hex, { size: opts.size });
    return pad(hex, { dir: "right", size: opts.size });
  }
  return hex;
}
function numberToHex(value_, opts = {}) {
  const { signed, size: size3 } = opts;
  const value = BigInt(value_);
  let maxValue;
  if (size3) {
    if (signed)
      maxValue = (1n << BigInt(size3) * 8n - 1n) - 1n;
    else
      maxValue = 2n ** (BigInt(size3) * 8n) - 1n;
  } else if (typeof value_ === "number") {
    maxValue = BigInt(Number.MAX_SAFE_INTEGER);
  }
  const minValue = typeof maxValue === "bigint" && signed ? -maxValue - 1n : 0;
  if (maxValue && value > maxValue || value < minValue) {
    const suffix = typeof value_ === "bigint" ? "n" : "";
    throw new IntegerOutOfRangeError({
      max: maxValue ? `${maxValue}${suffix}` : void 0,
      min: `${minValue}${suffix}`,
      signed,
      size: size3,
      value: `${value_}${suffix}`
    });
  }
  const hex = `0x${(signed && value < 0 ? (1n << BigInt(size3 * 8)) + BigInt(value) : value).toString(16)}`;
  if (size3)
    return pad(hex, { size: size3 });
  return hex;
}
function stringToHex(value_, opts = {}) {
  const value = encoder.encode(value_);
  return bytesToHex(value, opts);
}
var hexes, encoder;
var init_toHex = __esm({
  "node_modules/viem/_esm/utils/encoding/toHex.js"() {
    init_encoding();
    init_pad();
    init_fromHex();
    hexes = /* @__PURE__ */ Array.from({ length: 256 }, (_v, i) => i.toString(16).padStart(2, "0"));
    encoder = /* @__PURE__ */ new TextEncoder();
  }
});

// node_modules/viem/_esm/utils/encoding/toBytes.js
function toBytes(value, opts = {}) {
  if (typeof value === "number" || typeof value === "bigint")
    return numberToBytes(value, opts);
  if (typeof value === "boolean")
    return boolToBytes(value, opts);
  if (isHex(value))
    return hexToBytes(value, opts);
  return stringToBytes(value, opts);
}
function boolToBytes(value, opts = {}) {
  const bytes = new Uint8Array(1);
  bytes[0] = Number(value);
  if (typeof opts.size === "number") {
    assertSize(bytes, { size: opts.size });
    return pad(bytes, { size: opts.size });
  }
  return bytes;
}
function charCodeToBase16(char) {
  if (char >= charCodeMap.zero && char <= charCodeMap.nine)
    return char - charCodeMap.zero;
  if (char >= charCodeMap.A && char <= charCodeMap.F)
    return char - (charCodeMap.A - 10);
  if (char >= charCodeMap.a && char <= charCodeMap.f)
    return char - (charCodeMap.a - 10);
  return void 0;
}
function hexToBytes(hex_, opts = {}) {
  let hex = hex_;
  if (opts.size) {
    assertSize(hex, { size: opts.size });
    hex = pad(hex, { dir: "right", size: opts.size });
  }
  let hexString = hex.slice(2);
  if (hexString.length % 2)
    hexString = `0${hexString}`;
  const length = hexString.length / 2;
  const bytes = new Uint8Array(length);
  for (let index = 0, j = 0; index < length; index++) {
    const nibbleLeft = charCodeToBase16(hexString.charCodeAt(j++));
    const nibbleRight = charCodeToBase16(hexString.charCodeAt(j++));
    if (nibbleLeft === void 0 || nibbleRight === void 0) {
      throw new BaseError(`Invalid byte sequence ("${hexString[j - 2]}${hexString[j - 1]}" in "${hexString}").`);
    }
    bytes[index] = nibbleLeft * 16 + nibbleRight;
  }
  return bytes;
}
function numberToBytes(value, opts) {
  const hex = numberToHex(value, opts);
  return hexToBytes(hex);
}
function stringToBytes(value, opts = {}) {
  const bytes = encoder2.encode(value);
  if (typeof opts.size === "number") {
    assertSize(bytes, { size: opts.size });
    return pad(bytes, { dir: "right", size: opts.size });
  }
  return bytes;
}
var encoder2, charCodeMap;
var init_toBytes = __esm({
  "node_modules/viem/_esm/utils/encoding/toBytes.js"() {
    init_base();
    init_isHex();
    init_pad();
    init_fromHex();
    init_toHex();
    encoder2 = /* @__PURE__ */ new TextEncoder();
    charCodeMap = {
      zero: 48,
      nine: 57,
      A: 65,
      F: 70,
      a: 97,
      f: 102
    };
  }
});

// node_modules/viem/node_modules/@noble/hashes/esm/_u64.js
function fromBig(n, le = false) {
  if (le)
    return { h: Number(n & U32_MASK64), l: Number(n >> _32n & U32_MASK64) };
  return { h: Number(n >> _32n & U32_MASK64) | 0, l: Number(n & U32_MASK64) | 0 };
}
function split(lst, le = false) {
  const len = lst.length;
  let Ah = new Uint32Array(len);
  let Al = new Uint32Array(len);
  for (let i = 0; i < len; i++) {
    const { h, l } = fromBig(lst[i], le);
    [Ah[i], Al[i]] = [h, l];
  }
  return [Ah, Al];
}
var U32_MASK64, _32n, rotlSH, rotlSL, rotlBH, rotlBL;
var init_u64 = __esm({
  "node_modules/viem/node_modules/@noble/hashes/esm/_u64.js"() {
    U32_MASK64 = /* @__PURE__ */ BigInt(2 ** 32 - 1);
    _32n = /* @__PURE__ */ BigInt(32);
    rotlSH = (h, l, s) => h << s | l >>> 32 - s;
    rotlSL = (h, l, s) => l << s | h >>> 32 - s;
    rotlBH = (h, l, s) => l << s - 32 | h >>> 64 - s;
    rotlBL = (h, l, s) => h << s - 32 | l >>> 64 - s;
  }
});

// node_modules/viem/node_modules/@noble/hashes/esm/utils.js
function isBytes(a) {
  return a instanceof Uint8Array || ArrayBuffer.isView(a) && a.constructor.name === "Uint8Array";
}
function anumber(n) {
  if (!Number.isSafeInteger(n) || n < 0)
    throw new Error("positive integer expected, got " + n);
}
function abytes(b, ...lengths) {
  if (!isBytes(b))
    throw new Error("Uint8Array expected");
  if (lengths.length > 0 && !lengths.includes(b.length))
    throw new Error("Uint8Array expected of length " + lengths + ", got length=" + b.length);
}
function aexists(instance, checkFinished = true) {
  if (instance.destroyed)
    throw new Error("Hash instance has been destroyed");
  if (checkFinished && instance.finished)
    throw new Error("Hash#digest() has already been called");
}
function aoutput(out2, instance) {
  abytes(out2);
  const min = instance.outputLen;
  if (out2.length < min) {
    throw new Error("digestInto() expects output buffer of length at least " + min);
  }
}
function u32(arr) {
  return new Uint32Array(arr.buffer, arr.byteOffset, Math.floor(arr.byteLength / 4));
}
function clean(...arrays) {
  for (let i = 0; i < arrays.length; i++) {
    arrays[i].fill(0);
  }
}
function byteSwap(word2) {
  return word2 << 24 & 4278190080 | word2 << 8 & 16711680 | word2 >>> 8 & 65280 | word2 >>> 24 & 255;
}
function byteSwap32(arr) {
  for (let i = 0; i < arr.length; i++) {
    arr[i] = byteSwap(arr[i]);
  }
  return arr;
}
function utf8ToBytes(str) {
  if (typeof str !== "string")
    throw new Error("string expected");
  return new Uint8Array(new TextEncoder().encode(str));
}
function toBytes2(data) {
  if (typeof data === "string")
    data = utf8ToBytes(data);
  abytes(data);
  return data;
}
function createHasher(hashCons) {
  const hashC = (msg) => hashCons().update(toBytes2(msg)).digest();
  const tmp = hashCons();
  hashC.outputLen = tmp.outputLen;
  hashC.blockLen = tmp.blockLen;
  hashC.create = () => hashCons();
  return hashC;
}
var isLE, swap32IfBE, Hash;
var init_utils = __esm({
  "node_modules/viem/node_modules/@noble/hashes/esm/utils.js"() {
    isLE = /* @__PURE__ */ (() => new Uint8Array(new Uint32Array([287454020]).buffer)[0] === 68)();
    swap32IfBE = isLE ? (u) => u : byteSwap32;
    Hash = class {
    };
  }
});

// node_modules/viem/node_modules/@noble/hashes/esm/sha3.js
function keccakP(s, rounds = 24) {
  const B = new Uint32Array(5 * 2);
  for (let round = 24 - rounds; round < 24; round++) {
    for (let x = 0; x < 10; x++)
      B[x] = s[x] ^ s[x + 10] ^ s[x + 20] ^ s[x + 30] ^ s[x + 40];
    for (let x = 0; x < 10; x += 2) {
      const idx1 = (x + 8) % 10;
      const idx0 = (x + 2) % 10;
      const B0 = B[idx0];
      const B1 = B[idx0 + 1];
      const Th = rotlH(B0, B1, 1) ^ B[idx1];
      const Tl = rotlL(B0, B1, 1) ^ B[idx1 + 1];
      for (let y = 0; y < 50; y += 10) {
        s[x + y] ^= Th;
        s[x + y + 1] ^= Tl;
      }
    }
    let curH = s[2];
    let curL = s[3];
    for (let t = 0; t < 24; t++) {
      const shift = SHA3_ROTL[t];
      const Th = rotlH(curH, curL, shift);
      const Tl = rotlL(curH, curL, shift);
      const PI = SHA3_PI[t];
      curH = s[PI];
      curL = s[PI + 1];
      s[PI] = Th;
      s[PI + 1] = Tl;
    }
    for (let y = 0; y < 50; y += 10) {
      for (let x = 0; x < 10; x++)
        B[x] = s[y + x];
      for (let x = 0; x < 10; x++)
        s[y + x] ^= ~B[(x + 2) % 10] & B[(x + 4) % 10];
    }
    s[0] ^= SHA3_IOTA_H[round];
    s[1] ^= SHA3_IOTA_L[round];
  }
  clean(B);
}
var _0n, _1n, _2n, _7n, _256n, _0x71n, SHA3_PI, SHA3_ROTL, _SHA3_IOTA, IOTAS, SHA3_IOTA_H, SHA3_IOTA_L, rotlH, rotlL, Keccak, gen, keccak_256;
var init_sha3 = __esm({
  "node_modules/viem/node_modules/@noble/hashes/esm/sha3.js"() {
    init_u64();
    init_utils();
    _0n = BigInt(0);
    _1n = BigInt(1);
    _2n = BigInt(2);
    _7n = BigInt(7);
    _256n = BigInt(256);
    _0x71n = BigInt(113);
    SHA3_PI = [];
    SHA3_ROTL = [];
    _SHA3_IOTA = [];
    for (let round = 0, R2 = _1n, x = 1, y = 0; round < 24; round++) {
      [x, y] = [y, (2 * x + 3 * y) % 5];
      SHA3_PI.push(2 * (5 * y + x));
      SHA3_ROTL.push((round + 1) * (round + 2) / 2 % 64);
      let t = _0n;
      for (let j = 0; j < 7; j++) {
        R2 = (R2 << _1n ^ (R2 >> _7n) * _0x71n) % _256n;
        if (R2 & _2n)
          t ^= _1n << (_1n << /* @__PURE__ */ BigInt(j)) - _1n;
      }
      _SHA3_IOTA.push(t);
    }
    IOTAS = split(_SHA3_IOTA, true);
    SHA3_IOTA_H = IOTAS[0];
    SHA3_IOTA_L = IOTAS[1];
    rotlH = (h, l, s) => s > 32 ? rotlBH(h, l, s) : rotlSH(h, l, s);
    rotlL = (h, l, s) => s > 32 ? rotlBL(h, l, s) : rotlSL(h, l, s);
    Keccak = class _Keccak extends Hash {
      // NOTE: we accept arguments in bytes instead of bits here.
      constructor(blockLen, suffix, outputLen, enableXOF = false, rounds = 24) {
        super();
        this.pos = 0;
        this.posOut = 0;
        this.finished = false;
        this.destroyed = false;
        this.enableXOF = false;
        this.blockLen = blockLen;
        this.suffix = suffix;
        this.outputLen = outputLen;
        this.enableXOF = enableXOF;
        this.rounds = rounds;
        anumber(outputLen);
        if (!(0 < blockLen && blockLen < 200))
          throw new Error("only keccak-f1600 function is supported");
        this.state = new Uint8Array(200);
        this.state32 = u32(this.state);
      }
      clone() {
        return this._cloneInto();
      }
      keccak() {
        swap32IfBE(this.state32);
        keccakP(this.state32, this.rounds);
        swap32IfBE(this.state32);
        this.posOut = 0;
        this.pos = 0;
      }
      update(data) {
        aexists(this);
        data = toBytes2(data);
        abytes(data);
        const { blockLen, state } = this;
        const len = data.length;
        for (let pos = 0; pos < len; ) {
          const take = Math.min(blockLen - this.pos, len - pos);
          for (let i = 0; i < take; i++)
            state[this.pos++] ^= data[pos++];
          if (this.pos === blockLen)
            this.keccak();
        }
        return this;
      }
      finish() {
        if (this.finished)
          return;
        this.finished = true;
        const { state, suffix, pos, blockLen } = this;
        state[pos] ^= suffix;
        if ((suffix & 128) !== 0 && pos === blockLen - 1)
          this.keccak();
        state[blockLen - 1] ^= 128;
        this.keccak();
      }
      writeInto(out2) {
        aexists(this, false);
        abytes(out2);
        this.finish();
        const bufferOut = this.state;
        const { blockLen } = this;
        for (let pos = 0, len = out2.length; pos < len; ) {
          if (this.posOut >= blockLen)
            this.keccak();
          const take = Math.min(blockLen - this.posOut, len - pos);
          out2.set(bufferOut.subarray(this.posOut, this.posOut + take), pos);
          this.posOut += take;
          pos += take;
        }
        return out2;
      }
      xofInto(out2) {
        if (!this.enableXOF)
          throw new Error("XOF is not possible for this instance");
        return this.writeInto(out2);
      }
      xof(bytes) {
        anumber(bytes);
        return this.xofInto(new Uint8Array(bytes));
      }
      digestInto(out2) {
        aoutput(out2, this);
        if (this.finished)
          throw new Error("digest() was already called");
        this.writeInto(out2);
        this.destroy();
        return out2;
      }
      digest() {
        return this.digestInto(new Uint8Array(this.outputLen));
      }
      destroy() {
        this.destroyed = true;
        clean(this.state);
      }
      _cloneInto(to) {
        const { blockLen, suffix, outputLen, rounds, enableXOF } = this;
        to || (to = new _Keccak(blockLen, suffix, outputLen, enableXOF, rounds));
        to.state32.set(this.state32);
        to.pos = this.pos;
        to.posOut = this.posOut;
        to.finished = this.finished;
        to.rounds = rounds;
        to.suffix = suffix;
        to.outputLen = outputLen;
        to.enableXOF = enableXOF;
        to.destroyed = this.destroyed;
        return to;
      }
    };
    gen = (suffix, blockLen, outputLen) => createHasher(() => new Keccak(blockLen, suffix, outputLen));
    keccak_256 = /* @__PURE__ */ (() => gen(1, 136, 256 / 8))();
  }
});

// node_modules/viem/_esm/utils/hash/keccak256.js
function keccak256(value, to_) {
  const to = to_ || "hex";
  const bytes = keccak_256(isHex(value, { strict: false }) ? toBytes(value) : value);
  if (to === "bytes")
    return bytes;
  return toHex(bytes);
}
var init_keccak256 = __esm({
  "node_modules/viem/_esm/utils/hash/keccak256.js"() {
    init_sha3();
    init_isHex();
    init_toBytes();
    init_toHex();
  }
});

// node_modules/viem/_esm/utils/hash/hashSignature.js
function hashSignature(sig) {
  return hash(sig);
}
var hash;
var init_hashSignature = __esm({
  "node_modules/viem/_esm/utils/hash/hashSignature.js"() {
    init_toBytes();
    init_keccak256();
    hash = (value) => keccak256(toBytes(value));
  }
});

// node_modules/viem/_esm/utils/hash/normalizeSignature.js
function normalizeSignature(signature) {
  let active = true;
  let current = "";
  let level = 0;
  let result = "";
  let valid = false;
  for (let i = 0; i < signature.length; i++) {
    const char = signature[i];
    if (["(", ")", ","].includes(char))
      active = true;
    if (char === "(")
      level++;
    if (char === ")")
      level--;
    if (!active)
      continue;
    if (level === 0) {
      if (char === " " && ["event", "function", ""].includes(result))
        result = "";
      else {
        result += char;
        if (char === ")") {
          valid = true;
          break;
        }
      }
      continue;
    }
    if (char === " ") {
      if (signature[i - 1] !== "," && current !== "," && current !== ",(") {
        current = "";
        active = false;
      }
      continue;
    }
    result += char;
    current += char;
  }
  if (!valid)
    throw new BaseError("Unable to normalize signature.");
  return result;
}
var init_normalizeSignature = __esm({
  "node_modules/viem/_esm/utils/hash/normalizeSignature.js"() {
    init_base();
  }
});

// node_modules/viem/_esm/utils/hash/toSignature.js
var toSignature;
var init_toSignature = __esm({
  "node_modules/viem/_esm/utils/hash/toSignature.js"() {
    init_exports();
    init_normalizeSignature();
    toSignature = (def) => {
      const def_ = (() => {
        if (typeof def === "string")
          return def;
        return formatAbiItem(def);
      })();
      return normalizeSignature(def_);
    };
  }
});

// node_modules/viem/_esm/utils/hash/toSignatureHash.js
function toSignatureHash(fn) {
  return hashSignature(toSignature(fn));
}
var init_toSignatureHash = __esm({
  "node_modules/viem/_esm/utils/hash/toSignatureHash.js"() {
    init_hashSignature();
    init_toSignature();
  }
});

// node_modules/viem/_esm/utils/hash/toEventSelector.js
var toEventSelector;
var init_toEventSelector = __esm({
  "node_modules/viem/_esm/utils/hash/toEventSelector.js"() {
    init_toSignatureHash();
    toEventSelector = toSignatureHash;
  }
});

// node_modules/viem/_esm/errors/address.js
var InvalidAddressError;
var init_address = __esm({
  "node_modules/viem/_esm/errors/address.js"() {
    init_base();
    InvalidAddressError = class extends BaseError {
      constructor({ address }) {
        super(`Address "${address}" is invalid.`, {
          metaMessages: [
            "- Address must be a hex value of 20 bytes (40 hex characters).",
            "- Address must match its checksum counterpart."
          ],
          name: "InvalidAddressError"
        });
      }
    };
  }
});

// node_modules/viem/_esm/utils/lru.js
var LruMap;
var init_lru = __esm({
  "node_modules/viem/_esm/utils/lru.js"() {
    LruMap = class extends Map {
      constructor(size3) {
        super();
        Object.defineProperty(this, "maxSize", {
          enumerable: true,
          configurable: true,
          writable: true,
          value: void 0
        });
        this.maxSize = size3;
      }
      get(key) {
        const value = super.get(key);
        if (super.has(key)) {
          super.delete(key);
          super.set(key, value);
        }
        return value;
      }
      set(key, value) {
        if (super.has(key))
          super.delete(key);
        super.set(key, value);
        if (this.maxSize && this.size > this.maxSize) {
          const firstKey = super.keys().next().value;
          if (firstKey !== void 0)
            super.delete(firstKey);
        }
        return this;
      }
    };
  }
});

// node_modules/viem/_esm/utils/address/getAddress.js
function checksumAddress(address_, chainId) {
  if (checksumAddressCache.has(`${address_}.${chainId}`))
    return checksumAddressCache.get(`${address_}.${chainId}`);
  const hexAddress = chainId ? `${chainId}${address_.toLowerCase()}` : address_.substring(2).toLowerCase();
  const hash2 = keccak256(stringToBytes(hexAddress), "bytes");
  const address = (chainId ? hexAddress.substring(`${chainId}0x`.length) : hexAddress).split("");
  for (let i = 0; i < 40; i += 2) {
    if (hash2[i >> 1] >> 4 >= 8 && address[i]) {
      address[i] = address[i].toUpperCase();
    }
    if ((hash2[i >> 1] & 15) >= 8 && address[i + 1]) {
      address[i + 1] = address[i + 1].toUpperCase();
    }
  }
  const result = `0x${address.join("")}`;
  checksumAddressCache.set(`${address_}.${chainId}`, result);
  return result;
}
var checksumAddressCache;
var init_getAddress = __esm({
  "node_modules/viem/_esm/utils/address/getAddress.js"() {
    init_toBytes();
    init_keccak256();
    init_lru();
    checksumAddressCache = /* @__PURE__ */ new LruMap(8192);
  }
});

// node_modules/viem/_esm/utils/address/isAddress.js
function isAddress(address, options) {
  const { strict = true } = options ?? {};
  const cacheKey = `${address}.${strict}`;
  if (isAddressCache.has(cacheKey))
    return isAddressCache.get(cacheKey);
  const result = (() => {
    if (!addressRegex.test(address))
      return false;
    if (address.toLowerCase() === address)
      return true;
    if (strict)
      return checksumAddress(address) === address;
    return true;
  })();
  isAddressCache.set(cacheKey, result);
  return result;
}
var addressRegex, isAddressCache;
var init_isAddress = __esm({
  "node_modules/viem/_esm/utils/address/isAddress.js"() {
    init_lru();
    init_getAddress();
    addressRegex = /^0x[a-fA-F0-9]{40}$/;
    isAddressCache = /* @__PURE__ */ new LruMap(8192);
  }
});

// node_modules/viem/_esm/utils/data/concat.js
function concat(values) {
  if (typeof values[0] === "string")
    return concatHex(values);
  return concatBytes(values);
}
function concatBytes(values) {
  let length = 0;
  for (const arr of values) {
    length += arr.length;
  }
  const result = new Uint8Array(length);
  let offset = 0;
  for (const arr of values) {
    result.set(arr, offset);
    offset += arr.length;
  }
  return result;
}
function concatHex(values) {
  return `0x${values.reduce((acc, x) => acc + x.replace("0x", ""), "")}`;
}
var init_concat = __esm({
  "node_modules/viem/_esm/utils/data/concat.js"() {
  }
});

// node_modules/viem/_esm/utils/data/slice.js
function slice(value, start, end, { strict } = {}) {
  if (isHex(value, { strict: false }))
    return sliceHex(value, start, end, {
      strict
    });
  return sliceBytes(value, start, end, {
    strict
  });
}
function assertStartOffset(value, start) {
  if (typeof start === "number" && start > 0 && start > size(value) - 1)
    throw new SliceOffsetOutOfBoundsError({
      offset: start,
      position: "start",
      size: size(value)
    });
}
function assertEndOffset(value, start, end) {
  if (typeof start === "number" && typeof end === "number" && size(value) !== end - start) {
    throw new SliceOffsetOutOfBoundsError({
      offset: end,
      position: "end",
      size: size(value)
    });
  }
}
function sliceBytes(value_, start, end, { strict } = {}) {
  assertStartOffset(value_, start);
  const value = value_.slice(start, end);
  if (strict)
    assertEndOffset(value, start, end);
  return value;
}
function sliceHex(value_, start, end, { strict } = {}) {
  assertStartOffset(value_, start);
  const value = `0x${value_.replace("0x", "").slice((start ?? 0) * 2, (end ?? value_.length) * 2)}`;
  if (strict)
    assertEndOffset(value, start, end);
  return value;
}
var init_slice = __esm({
  "node_modules/viem/_esm/utils/data/slice.js"() {
    init_data();
    init_isHex();
    init_size();
  }
});

// node_modules/viem/_esm/utils/regex.js
var integerRegex;
var init_regex2 = __esm({
  "node_modules/viem/_esm/utils/regex.js"() {
    integerRegex = /^(u?int)(8|16|24|32|40|48|56|64|72|80|88|96|104|112|120|128|136|144|152|160|168|176|184|192|200|208|216|224|232|240|248|256)?$/;
  }
});

// node_modules/viem/_esm/utils/abi/encodeAbiParameters.js
function encodeAbiParameters(params, values) {
  if (params.length !== values.length)
    throw new AbiEncodingLengthMismatchError({
      expectedLength: params.length,
      givenLength: values.length
    });
  const preparedParams = prepareParams({
    params,
    values
  });
  const data = encodeParams(preparedParams);
  if (data.length === 0)
    return "0x";
  return data;
}
function prepareParams({ params, values }) {
  const preparedParams = [];
  for (let i = 0; i < params.length; i++) {
    preparedParams.push(prepareParam({ param: params[i], value: values[i] }));
  }
  return preparedParams;
}
function prepareParam({ param, value }) {
  const arrayComponents = getArrayComponents(param.type);
  if (arrayComponents) {
    const [length, type] = arrayComponents;
    return encodeArray(value, { length, param: { ...param, type } });
  }
  if (param.type === "tuple") {
    return encodeTuple(value, {
      param
    });
  }
  if (param.type === "address") {
    return encodeAddress(value);
  }
  if (param.type === "bool") {
    return encodeBool(value);
  }
  if (param.type.startsWith("uint") || param.type.startsWith("int")) {
    const signed = param.type.startsWith("int");
    const [, , size3 = "256"] = integerRegex.exec(param.type) ?? [];
    return encodeNumber(value, {
      signed,
      size: Number(size3)
    });
  }
  if (param.type.startsWith("bytes")) {
    return encodeBytes(value, { param });
  }
  if (param.type === "string") {
    return encodeString(value);
  }
  throw new InvalidAbiEncodingTypeError(param.type, {
    docsPath: "/docs/contract/encodeAbiParameters"
  });
}
function encodeParams(preparedParams) {
  let staticSize = 0;
  for (let i = 0; i < preparedParams.length; i++) {
    const { dynamic, encoded } = preparedParams[i];
    if (dynamic)
      staticSize += 32;
    else
      staticSize += size(encoded);
  }
  const staticParams = [];
  const dynamicParams = [];
  let dynamicSize = 0;
  for (let i = 0; i < preparedParams.length; i++) {
    const { dynamic, encoded } = preparedParams[i];
    if (dynamic) {
      staticParams.push(numberToHex(staticSize + dynamicSize, { size: 32 }));
      dynamicParams.push(encoded);
      dynamicSize += size(encoded);
    } else {
      staticParams.push(encoded);
    }
  }
  return concat([...staticParams, ...dynamicParams]);
}
function encodeAddress(value) {
  if (!isAddress(value))
    throw new InvalidAddressError({ address: value });
  return { dynamic: false, encoded: padHex(value.toLowerCase()) };
}
function encodeArray(value, { length, param }) {
  const dynamic = length === null;
  if (!Array.isArray(value))
    throw new InvalidArrayError(value);
  if (!dynamic && value.length !== length)
    throw new AbiEncodingArrayLengthMismatchError({
      expectedLength: length,
      givenLength: value.length,
      type: `${param.type}[${length}]`
    });
  let dynamicChild = false;
  const preparedParams = [];
  for (let i = 0; i < value.length; i++) {
    const preparedParam = prepareParam({ param, value: value[i] });
    if (preparedParam.dynamic)
      dynamicChild = true;
    preparedParams.push(preparedParam);
  }
  if (dynamic || dynamicChild) {
    const data = encodeParams(preparedParams);
    if (dynamic) {
      const length2 = numberToHex(preparedParams.length, { size: 32 });
      return {
        dynamic: true,
        encoded: preparedParams.length > 0 ? concat([length2, data]) : length2
      };
    }
    if (dynamicChild)
      return { dynamic: true, encoded: data };
  }
  return {
    dynamic: false,
    encoded: concat(preparedParams.map(({ encoded }) => encoded))
  };
}
function encodeBytes(value, { param }) {
  const [, paramSize] = param.type.split("bytes");
  const bytesSize = size(value);
  if (!paramSize) {
    let value_ = value;
    if (bytesSize % 32 !== 0)
      value_ = padHex(value_, {
        dir: "right",
        size: Math.ceil((value.length - 2) / 2 / 32) * 32
      });
    return {
      dynamic: true,
      encoded: concat([padHex(numberToHex(bytesSize, { size: 32 })), value_])
    };
  }
  if (bytesSize !== Number.parseInt(paramSize, 10))
    throw new AbiEncodingBytesSizeMismatchError({
      expectedSize: Number.parseInt(paramSize, 10),
      value
    });
  return { dynamic: false, encoded: padHex(value, { dir: "right" }) };
}
function encodeBool(value) {
  if (typeof value !== "boolean")
    throw new BaseError(`Invalid boolean value: "${value}" (type: ${typeof value}). Expected: \`true\` or \`false\`.`);
  return { dynamic: false, encoded: padHex(boolToHex(value)) };
}
function encodeNumber(value, { signed, size: size3 = 256 }) {
  if (typeof size3 === "number") {
    const max = 2n ** (BigInt(size3) - (signed ? 1n : 0n)) - 1n;
    const min = signed ? -max - 1n : 0n;
    if (value > max || value < min)
      throw new IntegerOutOfRangeError({
        max: max.toString(),
        min: min.toString(),
        signed,
        size: size3 / 8,
        value: value.toString()
      });
  }
  return {
    dynamic: false,
    encoded: numberToHex(value, {
      size: 32,
      signed
    })
  };
}
function encodeString(value) {
  const hexValue = stringToHex(value);
  const partsLength = Math.ceil(size(hexValue) / 32);
  const parts = [];
  for (let i = 0; i < partsLength; i++) {
    parts.push(padHex(slice(hexValue, i * 32, (i + 1) * 32), {
      dir: "right"
    }));
  }
  return {
    dynamic: true,
    encoded: concat([
      padHex(numberToHex(size(hexValue), { size: 32 })),
      ...parts
    ])
  };
}
function encodeTuple(value, { param }) {
  let dynamic = false;
  const preparedParams = [];
  for (let i = 0; i < param.components.length; i++) {
    const param_ = param.components[i];
    const index = Array.isArray(value) ? i : param_.name;
    const preparedParam = prepareParam({
      param: param_,
      value: value[index]
    });
    preparedParams.push(preparedParam);
    if (preparedParam.dynamic)
      dynamic = true;
  }
  return {
    dynamic,
    encoded: dynamic ? encodeParams(preparedParams) : concat(preparedParams.map(({ encoded }) => encoded))
  };
}
function getArrayComponents(type) {
  const matches = type.match(/^(.*)\[(\d+)?\]$/);
  return matches ? (
    // Return `null` if the array is dynamic.
    [matches[2] ? Number(matches[2]) : null, matches[1]]
  ) : void 0;
}
var init_encodeAbiParameters = __esm({
  "node_modules/viem/_esm/utils/abi/encodeAbiParameters.js"() {
    init_abi();
    init_address();
    init_base();
    init_encoding();
    init_isAddress();
    init_concat();
    init_pad();
    init_size();
    init_slice();
    init_toHex();
    init_regex2();
  }
});

// node_modules/viem/_esm/utils/hash/toFunctionSelector.js
var toFunctionSelector;
var init_toFunctionSelector = __esm({
  "node_modules/viem/_esm/utils/hash/toFunctionSelector.js"() {
    init_slice();
    init_toSignatureHash();
    toFunctionSelector = (fn) => slice(toSignatureHash(fn), 0, 4);
  }
});

// node_modules/viem/_esm/utils/abi/getAbiItem.js
function getAbiItem(parameters) {
  const { abi, args = [], name } = parameters;
  const isSelector = isHex(name, { strict: false });
  const abiItems = abi.filter((abiItem) => {
    if (isSelector) {
      if (abiItem.type === "function")
        return toFunctionSelector(abiItem) === name;
      if (abiItem.type === "event")
        return toEventSelector(abiItem) === name;
      return false;
    }
    return "name" in abiItem && abiItem.name === name;
  });
  if (abiItems.length === 0)
    return void 0;
  if (abiItems.length === 1)
    return abiItems[0];
  let matchedAbiItem;
  for (const abiItem of abiItems) {
    if (!("inputs" in abiItem))
      continue;
    if (!args || args.length === 0) {
      if (!abiItem.inputs || abiItem.inputs.length === 0)
        return abiItem;
      continue;
    }
    if (!abiItem.inputs)
      continue;
    if (abiItem.inputs.length === 0)
      continue;
    if (abiItem.inputs.length !== args.length)
      continue;
    const matched = args.every((arg, index) => {
      const abiParameter = "inputs" in abiItem && abiItem.inputs[index];
      if (!abiParameter)
        return false;
      return isArgOfType(arg, abiParameter);
    });
    if (matched) {
      if (matchedAbiItem && "inputs" in matchedAbiItem && matchedAbiItem.inputs) {
        const ambiguousTypes = getAmbiguousTypes(abiItem.inputs, matchedAbiItem.inputs, args);
        if (ambiguousTypes)
          throw new AbiItemAmbiguityError({
            abiItem,
            type: ambiguousTypes[0]
          }, {
            abiItem: matchedAbiItem,
            type: ambiguousTypes[1]
          });
      }
      matchedAbiItem = abiItem;
    }
  }
  if (matchedAbiItem)
    return matchedAbiItem;
  return abiItems[0];
}
function isArgOfType(arg, abiParameter) {
  const argType = typeof arg;
  const abiParameterType = abiParameter.type;
  switch (abiParameterType) {
    case "address":
      return isAddress(arg, { strict: false });
    case "bool":
      return argType === "boolean";
    case "function":
      return argType === "string";
    case "string":
      return argType === "string";
    default: {
      if (abiParameterType === "tuple" && "components" in abiParameter)
        return Object.values(abiParameter.components).every((component, index) => {
          return argType === "object" && isArgOfType(Object.values(arg)[index], component);
        });
      if (/^u?int(8|16|24|32|40|48|56|64|72|80|88|96|104|112|120|128|136|144|152|160|168|176|184|192|200|208|216|224|232|240|248|256)?$/.test(abiParameterType))
        return argType === "number" || argType === "bigint";
      if (/^bytes([1-9]|1[0-9]|2[0-9]|3[0-2])?$/.test(abiParameterType))
        return argType === "string" || arg instanceof Uint8Array;
      if (/[a-z]+[1-9]{0,3}(\[[0-9]{0,}\])+$/.test(abiParameterType)) {
        return Array.isArray(arg) && arg.every((x) => isArgOfType(x, {
          ...abiParameter,
          // Pop off `[]` or `[M]` from end of type
          type: abiParameterType.replace(/(\[[0-9]{0,}\])$/, "")
        }));
      }
      return false;
    }
  }
}
function getAmbiguousTypes(sourceParameters, targetParameters, args) {
  for (const parameterIndex in sourceParameters) {
    const sourceParameter = sourceParameters[parameterIndex];
    const targetParameter = targetParameters[parameterIndex];
    if (sourceParameter.type === "tuple" && targetParameter.type === "tuple" && "components" in sourceParameter && "components" in targetParameter)
      return getAmbiguousTypes(sourceParameter.components, targetParameter.components, args[parameterIndex]);
    const types = [sourceParameter.type, targetParameter.type];
    const ambiguous = (() => {
      if (types.includes("address") && types.includes("bytes20"))
        return true;
      if (types.includes("address") && types.includes("string"))
        return isAddress(args[parameterIndex], { strict: false });
      if (types.includes("address") && types.includes("bytes"))
        return isAddress(args[parameterIndex], { strict: false });
      return false;
    })();
    if (ambiguous)
      return types;
  }
  return;
}
var init_getAbiItem = __esm({
  "node_modules/viem/_esm/utils/abi/getAbiItem.js"() {
    init_abi();
    init_isHex();
    init_isAddress();
    init_toEventSelector();
    init_toFunctionSelector();
  }
});

// node_modules/viem/_esm/utils/abi/prepareEncodeFunctionData.js
function prepareEncodeFunctionData(parameters) {
  const { abi, args, functionName } = parameters;
  let abiItem = abi[0];
  if (functionName) {
    const item = getAbiItem({
      abi,
      args,
      name: functionName
    });
    if (!item)
      throw new AbiFunctionNotFoundError(functionName, { docsPath });
    abiItem = item;
  }
  if (abiItem.type !== "function")
    throw new AbiFunctionNotFoundError(void 0, { docsPath });
  return {
    abi: [abiItem],
    functionName: toFunctionSelector(formatAbiItem2(abiItem))
  };
}
var docsPath;
var init_prepareEncodeFunctionData = __esm({
  "node_modules/viem/_esm/utils/abi/prepareEncodeFunctionData.js"() {
    init_abi();
    init_toFunctionSelector();
    init_formatAbiItem2();
    init_getAbiItem();
    docsPath = "/docs/contract/encodeFunctionData";
  }
});

// node_modules/viem/_esm/utils/abi/encodeFunctionData.js
function encodeFunctionData(parameters) {
  const { args } = parameters;
  const { abi, functionName } = (() => {
    if (parameters.abi.length === 1 && parameters.functionName?.startsWith("0x"))
      return parameters;
    return prepareEncodeFunctionData(parameters);
  })();
  const abiItem = abi[0];
  const signature = functionName;
  const data = "inputs" in abiItem && abiItem.inputs ? encodeAbiParameters(abiItem.inputs, args ?? []) : void 0;
  return concatHex([signature, data ?? "0x"]);
}
var init_encodeFunctionData = __esm({
  "node_modules/viem/_esm/utils/abi/encodeFunctionData.js"() {
    init_concat();
    init_encodeAbiParameters();
    init_prepareEncodeFunctionData();
  }
});

// node_modules/viem/_esm/errors/cursor.js
var NegativeOffsetError, PositionOutOfBoundsError, RecursiveReadLimitExceededError;
var init_cursor = __esm({
  "node_modules/viem/_esm/errors/cursor.js"() {
    init_base();
    NegativeOffsetError = class extends BaseError {
      constructor({ offset }) {
        super(`Offset \`${offset}\` cannot be negative.`, {
          name: "NegativeOffsetError"
        });
      }
    };
    PositionOutOfBoundsError = class extends BaseError {
      constructor({ length, position }) {
        super(`Position \`${position}\` is out of bounds (\`0 < position < ${length}\`).`, { name: "PositionOutOfBoundsError" });
      }
    };
    RecursiveReadLimitExceededError = class extends BaseError {
      constructor({ count, limit }) {
        super(`Recursive read limit of \`${limit}\` exceeded (recursive read count: \`${count}\`).`, { name: "RecursiveReadLimitExceededError" });
      }
    };
  }
});

// node_modules/viem/_esm/utils/cursor.js
function createCursor(bytes, { recursiveReadLimit = 8192 } = {}) {
  const cursor = Object.create(staticCursor);
  cursor.bytes = bytes;
  cursor.dataView = new DataView(bytes.buffer ?? bytes, bytes.byteOffset, bytes.byteLength);
  cursor.positionReadCount = /* @__PURE__ */ new Map();
  cursor.recursiveReadLimit = recursiveReadLimit;
  return cursor;
}
var staticCursor;
var init_cursor2 = __esm({
  "node_modules/viem/_esm/utils/cursor.js"() {
    init_cursor();
    staticCursor = {
      bytes: new Uint8Array(),
      dataView: new DataView(new ArrayBuffer(0)),
      position: 0,
      positionReadCount: /* @__PURE__ */ new Map(),
      recursiveReadCount: 0,
      recursiveReadLimit: Number.POSITIVE_INFINITY,
      assertReadLimit() {
        if (this.recursiveReadCount >= this.recursiveReadLimit)
          throw new RecursiveReadLimitExceededError({
            count: this.recursiveReadCount + 1,
            limit: this.recursiveReadLimit
          });
      },
      assertPosition(position) {
        if (position < 0 || position > this.bytes.length - 1)
          throw new PositionOutOfBoundsError({
            length: this.bytes.length,
            position
          });
      },
      decrementPosition(offset) {
        if (offset < 0)
          throw new NegativeOffsetError({ offset });
        const position = this.position - offset;
        this.assertPosition(position);
        this.position = position;
      },
      getReadCount(position) {
        return this.positionReadCount.get(position || this.position) || 0;
      },
      incrementPosition(offset) {
        if (offset < 0)
          throw new NegativeOffsetError({ offset });
        const position = this.position + offset;
        this.assertPosition(position);
        this.position = position;
      },
      inspectByte(position_) {
        const position = position_ ?? this.position;
        this.assertPosition(position);
        return this.bytes[position];
      },
      inspectBytes(length, position_) {
        const position = position_ ?? this.position;
        this.assertPosition(position + length - 1);
        return this.bytes.subarray(position, position + length);
      },
      inspectUint8(position_) {
        const position = position_ ?? this.position;
        this.assertPosition(position);
        return this.bytes[position];
      },
      inspectUint16(position_) {
        const position = position_ ?? this.position;
        this.assertPosition(position + 1);
        return this.dataView.getUint16(position);
      },
      inspectUint24(position_) {
        const position = position_ ?? this.position;
        this.assertPosition(position + 2);
        return (this.dataView.getUint16(position) << 8) + this.dataView.getUint8(position + 2);
      },
      inspectUint32(position_) {
        const position = position_ ?? this.position;
        this.assertPosition(position + 3);
        return this.dataView.getUint32(position);
      },
      pushByte(byte) {
        this.assertPosition(this.position);
        this.bytes[this.position] = byte;
        this.position++;
      },
      pushBytes(bytes) {
        this.assertPosition(this.position + bytes.length - 1);
        this.bytes.set(bytes, this.position);
        this.position += bytes.length;
      },
      pushUint8(value) {
        this.assertPosition(this.position);
        this.bytes[this.position] = value;
        this.position++;
      },
      pushUint16(value) {
        this.assertPosition(this.position + 1);
        this.dataView.setUint16(this.position, value);
        this.position += 2;
      },
      pushUint24(value) {
        this.assertPosition(this.position + 2);
        this.dataView.setUint16(this.position, value >> 8);
        this.dataView.setUint8(this.position + 2, value & ~4294967040);
        this.position += 3;
      },
      pushUint32(value) {
        this.assertPosition(this.position + 3);
        this.dataView.setUint32(this.position, value);
        this.position += 4;
      },
      readByte() {
        this.assertReadLimit();
        this._touch();
        const value = this.inspectByte();
        this.position++;
        return value;
      },
      readBytes(length, size3) {
        this.assertReadLimit();
        this._touch();
        const value = this.inspectBytes(length);
        this.position += size3 ?? length;
        return value;
      },
      readUint8() {
        this.assertReadLimit();
        this._touch();
        const value = this.inspectUint8();
        this.position += 1;
        return value;
      },
      readUint16() {
        this.assertReadLimit();
        this._touch();
        const value = this.inspectUint16();
        this.position += 2;
        return value;
      },
      readUint24() {
        this.assertReadLimit();
        this._touch();
        const value = this.inspectUint24();
        this.position += 3;
        return value;
      },
      readUint32() {
        this.assertReadLimit();
        this._touch();
        const value = this.inspectUint32();
        this.position += 4;
        return value;
      },
      get remaining() {
        return this.bytes.length - this.position;
      },
      setPosition(position) {
        const oldPosition = this.position;
        this.assertPosition(position);
        this.position = position;
        return () => this.position = oldPosition;
      },
      _touch() {
        if (this.recursiveReadLimit === Number.POSITIVE_INFINITY)
          return;
        const count = this.getReadCount();
        this.positionReadCount.set(this.position, count + 1);
        if (count > 0)
          this.recursiveReadCount++;
      }
    };
  }
});

// node_modules/viem/_esm/utils/encoding/fromBytes.js
function bytesToBigInt(bytes, opts = {}) {
  if (typeof opts.size !== "undefined")
    assertSize(bytes, { size: opts.size });
  const hex = bytesToHex(bytes, opts);
  return hexToBigInt(hex, opts);
}
function bytesToBool(bytes_, opts = {}) {
  let bytes = bytes_;
  if (typeof opts.size !== "undefined") {
    assertSize(bytes, { size: opts.size });
    bytes = trim(bytes);
  }
  if (bytes.length > 1 || bytes[0] > 1)
    throw new InvalidBytesBooleanError(bytes);
  return Boolean(bytes[0]);
}
function bytesToNumber(bytes, opts = {}) {
  if (typeof opts.size !== "undefined")
    assertSize(bytes, { size: opts.size });
  const hex = bytesToHex(bytes, opts);
  return hexToNumber(hex, opts);
}
function bytesToString(bytes_, opts = {}) {
  let bytes = bytes_;
  if (typeof opts.size !== "undefined") {
    assertSize(bytes, { size: opts.size });
    bytes = trim(bytes, { dir: "right" });
  }
  return new TextDecoder().decode(bytes);
}
var init_fromBytes = __esm({
  "node_modules/viem/_esm/utils/encoding/fromBytes.js"() {
    init_encoding();
    init_trim();
    init_fromHex();
    init_toHex();
  }
});

// node_modules/viem/_esm/utils/abi/decodeAbiParameters.js
function decodeAbiParameters(params, data) {
  const bytes = typeof data === "string" ? hexToBytes(data) : data;
  const cursor = createCursor(bytes);
  if (size(bytes) === 0 && params.length > 0)
    throw new AbiDecodingZeroDataError();
  if (size(data) && size(data) < 32)
    throw new AbiDecodingDataSizeTooSmallError({
      data: typeof data === "string" ? data : bytesToHex(data),
      params,
      size: size(data)
    });
  let consumed = 0;
  const values = [];
  for (let i = 0; i < params.length; ++i) {
    const param = params[i];
    cursor.setPosition(consumed);
    const [data2, consumed_] = decodeParameter(cursor, param, {
      staticPosition: 0
    });
    consumed += consumed_;
    values.push(data2);
  }
  return values;
}
function decodeParameter(cursor, param, { staticPosition }) {
  const arrayComponents = getArrayComponents(param.type);
  if (arrayComponents) {
    const [length, type] = arrayComponents;
    return decodeArray(cursor, { ...param, type }, { length, staticPosition });
  }
  if (param.type === "tuple")
    return decodeTuple(cursor, param, { staticPosition });
  if (param.type === "address")
    return decodeAddress(cursor);
  if (param.type === "bool")
    return decodeBool(cursor);
  if (param.type.startsWith("bytes"))
    return decodeBytes(cursor, param, { staticPosition });
  if (param.type.startsWith("uint") || param.type.startsWith("int"))
    return decodeNumber(cursor, param);
  if (param.type === "string")
    return decodeString(cursor, { staticPosition });
  throw new InvalidAbiDecodingTypeError(param.type, {
    docsPath: "/docs/contract/decodeAbiParameters"
  });
}
function decodeAddress(cursor) {
  const value = cursor.readBytes(32);
  return [checksumAddress(bytesToHex(sliceBytes(value, -20))), 32];
}
function decodeArray(cursor, param, { length, staticPosition }) {
  if (!length) {
    const offset = bytesToNumber(cursor.readBytes(sizeOfOffset));
    const start = staticPosition + offset;
    const startOfData = start + sizeOfLength;
    cursor.setPosition(start);
    const length2 = bytesToNumber(cursor.readBytes(sizeOfLength));
    const dynamicChild = hasDynamicChild(param);
    let consumed2 = 0;
    const value2 = [];
    for (let i = 0; i < length2; ++i) {
      cursor.setPosition(startOfData + (dynamicChild ? i * 32 : consumed2));
      const [data, consumed_] = decodeParameter(cursor, param, {
        staticPosition: startOfData
      });
      consumed2 += consumed_;
      value2.push(data);
    }
    cursor.setPosition(staticPosition + 32);
    return [value2, 32];
  }
  if (hasDynamicChild(param)) {
    const offset = bytesToNumber(cursor.readBytes(sizeOfOffset));
    const start = staticPosition + offset;
    const value2 = [];
    for (let i = 0; i < length; ++i) {
      cursor.setPosition(start + i * 32);
      const [data] = decodeParameter(cursor, param, {
        staticPosition: start
      });
      value2.push(data);
    }
    cursor.setPosition(staticPosition + 32);
    return [value2, 32];
  }
  let consumed = 0;
  const value = [];
  for (let i = 0; i < length; ++i) {
    const [data, consumed_] = decodeParameter(cursor, param, {
      staticPosition: staticPosition + consumed
    });
    consumed += consumed_;
    value.push(data);
  }
  return [value, consumed];
}
function decodeBool(cursor) {
  return [bytesToBool(cursor.readBytes(32), { size: 32 }), 32];
}
function decodeBytes(cursor, param, { staticPosition }) {
  const [_, size3] = param.type.split("bytes");
  if (!size3) {
    const offset = bytesToNumber(cursor.readBytes(32));
    cursor.setPosition(staticPosition + offset);
    const length = bytesToNumber(cursor.readBytes(32));
    if (length === 0) {
      cursor.setPosition(staticPosition + 32);
      return ["0x", 32];
    }
    const data = cursor.readBytes(length);
    cursor.setPosition(staticPosition + 32);
    return [bytesToHex(data), 32];
  }
  const value = bytesToHex(cursor.readBytes(Number.parseInt(size3, 10), 32));
  return [value, 32];
}
function decodeNumber(cursor, param) {
  const signed = param.type.startsWith("int");
  const size3 = Number.parseInt(param.type.split("int")[1] || "256", 10);
  const value = cursor.readBytes(32);
  return [
    size3 > 48 ? bytesToBigInt(value, { signed }) : bytesToNumber(value, { signed }),
    32
  ];
}
function decodeTuple(cursor, param, { staticPosition }) {
  const hasUnnamedChild = param.components.length === 0 || param.components.some(({ name }) => !name);
  const value = hasUnnamedChild ? [] : {};
  let consumed = 0;
  if (hasDynamicChild(param)) {
    const offset = bytesToNumber(cursor.readBytes(sizeOfOffset));
    const start = staticPosition + offset;
    for (let i = 0; i < param.components.length; ++i) {
      const component = param.components[i];
      cursor.setPosition(start + consumed);
      const [data, consumed_] = decodeParameter(cursor, component, {
        staticPosition: start
      });
      consumed += consumed_;
      value[hasUnnamedChild ? i : component?.name] = data;
    }
    cursor.setPosition(staticPosition + 32);
    return [value, 32];
  }
  for (let i = 0; i < param.components.length; ++i) {
    const component = param.components[i];
    const [data, consumed_] = decodeParameter(cursor, component, {
      staticPosition
    });
    value[hasUnnamedChild ? i : component?.name] = data;
    consumed += consumed_;
  }
  return [value, consumed];
}
function decodeString(cursor, { staticPosition }) {
  const offset = bytesToNumber(cursor.readBytes(32));
  const start = staticPosition + offset;
  cursor.setPosition(start);
  const length = bytesToNumber(cursor.readBytes(32));
  if (length === 0) {
    cursor.setPosition(staticPosition + 32);
    return ["", 32];
  }
  const data = cursor.readBytes(length, 32);
  const value = bytesToString(trim(data));
  cursor.setPosition(staticPosition + 32);
  return [value, 32];
}
function hasDynamicChild(param) {
  const { type } = param;
  if (type === "string")
    return true;
  if (type === "bytes")
    return true;
  if (type.endsWith("[]"))
    return true;
  if (type === "tuple")
    return param.components?.some(hasDynamicChild);
  const arrayComponents = getArrayComponents(param.type);
  if (arrayComponents && hasDynamicChild({ ...param, type: arrayComponents[1] }))
    return true;
  return false;
}
var sizeOfLength, sizeOfOffset;
var init_decodeAbiParameters = __esm({
  "node_modules/viem/_esm/utils/abi/decodeAbiParameters.js"() {
    init_abi();
    init_getAddress();
    init_cursor2();
    init_size();
    init_slice();
    init_trim();
    init_fromBytes();
    init_toBytes();
    init_toHex();
    init_encodeAbiParameters();
    sizeOfLength = 32;
    sizeOfOffset = 32;
  }
});

// node_modules/viem/_esm/utils/abi/decodeFunctionResult.js
function decodeFunctionResult(parameters) {
  const { abi, args, functionName, data } = parameters;
  let abiItem = abi[0];
  if (functionName) {
    const item = getAbiItem({ abi, args, name: functionName });
    if (!item)
      throw new AbiFunctionNotFoundError(functionName, { docsPath: docsPath2 });
    abiItem = item;
  }
  if (abiItem.type !== "function")
    throw new AbiFunctionNotFoundError(void 0, { docsPath: docsPath2 });
  if (!abiItem.outputs)
    throw new AbiFunctionOutputsNotFoundError(abiItem.name, { docsPath: docsPath2 });
  const values = decodeAbiParameters(abiItem.outputs, data);
  if (values && values.length > 1)
    return values;
  if (values && values.length === 1)
    return values[0];
  return void 0;
}
var docsPath2;
var init_decodeFunctionResult = __esm({
  "node_modules/viem/_esm/utils/abi/decodeFunctionResult.js"() {
    init_abi();
    init_decodeAbiParameters();
    init_getAbiItem();
    docsPath2 = "/docs/contract/decodeFunctionResult";
  }
});

// node_modules/viem/_esm/constants/abis.js
var universalResolverErrors, universalResolverResolveAbi, universalResolverReverseAbi, erc20Abi;
var init_abis = __esm({
  "node_modules/viem/_esm/constants/abis.js"() {
    universalResolverErrors = [
      {
        inputs: [
          {
            name: "dns",
            type: "bytes"
          }
        ],
        name: "DNSDecodingFailed",
        type: "error"
      },
      {
        inputs: [
          {
            name: "ens",
            type: "string"
          }
        ],
        name: "DNSEncodingFailed",
        type: "error"
      },
      {
        inputs: [],
        name: "EmptyAddress",
        type: "error"
      },
      {
        inputs: [
          {
            name: "status",
            type: "uint16"
          },
          {
            name: "message",
            type: "string"
          }
        ],
        name: "HttpError",
        type: "error"
      },
      {
        inputs: [],
        name: "InvalidBatchGatewayResponse",
        type: "error"
      },
      {
        inputs: [
          {
            name: "errorData",
            type: "bytes"
          }
        ],
        name: "ResolverError",
        type: "error"
      },
      {
        inputs: [
          {
            name: "name",
            type: "bytes"
          },
          {
            name: "resolver",
            type: "address"
          }
        ],
        name: "ResolverNotContract",
        type: "error"
      },
      {
        inputs: [
          {
            name: "name",
            type: "bytes"
          }
        ],
        name: "ResolverNotFound",
        type: "error"
      },
      {
        inputs: [
          {
            name: "primary",
            type: "string"
          },
          {
            name: "primaryAddress",
            type: "bytes"
          }
        ],
        name: "ReverseAddressMismatch",
        type: "error"
      },
      {
        inputs: [
          {
            internalType: "bytes4",
            name: "selector",
            type: "bytes4"
          }
        ],
        name: "UnsupportedResolverProfile",
        type: "error"
      }
    ];
    universalResolverResolveAbi = [
      ...universalResolverErrors,
      {
        name: "resolveWithGateways",
        type: "function",
        stateMutability: "view",
        inputs: [
          { name: "name", type: "bytes" },
          { name: "data", type: "bytes" },
          { name: "gateways", type: "string[]" }
        ],
        outputs: [
          { name: "", type: "bytes" },
          { name: "address", type: "address" }
        ]
      }
    ];
    universalResolverReverseAbi = [
      ...universalResolverErrors,
      {
        name: "reverseWithGateways",
        type: "function",
        stateMutability: "view",
        inputs: [
          { type: "bytes", name: "reverseName" },
          { type: "uint256", name: "coinType" },
          { type: "string[]", name: "gateways" }
        ],
        outputs: [
          { type: "string", name: "resolvedName" },
          { type: "address", name: "resolver" },
          { type: "address", name: "reverseResolver" }
        ]
      }
    ];
    erc20Abi = [
      {
        type: "event",
        name: "Approval",
        inputs: [
          {
            indexed: true,
            name: "owner",
            type: "address"
          },
          {
            indexed: true,
            name: "spender",
            type: "address"
          },
          {
            indexed: false,
            name: "value",
            type: "uint256"
          }
        ]
      },
      {
        type: "event",
        name: "Transfer",
        inputs: [
          {
            indexed: true,
            name: "from",
            type: "address"
          },
          {
            indexed: true,
            name: "to",
            type: "address"
          },
          {
            indexed: false,
            name: "value",
            type: "uint256"
          }
        ]
      },
      {
        type: "function",
        name: "allowance",
        stateMutability: "view",
        inputs: [
          {
            name: "owner",
            type: "address"
          },
          {
            name: "spender",
            type: "address"
          }
        ],
        outputs: [
          {
            type: "uint256"
          }
        ]
      },
      {
        type: "function",
        name: "approve",
        stateMutability: "nonpayable",
        inputs: [
          {
            name: "spender",
            type: "address"
          },
          {
            name: "amount",
            type: "uint256"
          }
        ],
        outputs: [
          {
            type: "bool"
          }
        ]
      },
      {
        type: "function",
        name: "balanceOf",
        stateMutability: "view",
        inputs: [
          {
            name: "account",
            type: "address"
          }
        ],
        outputs: [
          {
            type: "uint256"
          }
        ]
      },
      {
        type: "function",
        name: "decimals",
        stateMutability: "view",
        inputs: [],
        outputs: [
          {
            type: "uint8"
          }
        ]
      },
      {
        type: "function",
        name: "name",
        stateMutability: "view",
        inputs: [],
        outputs: [
          {
            type: "string"
          }
        ]
      },
      {
        type: "function",
        name: "symbol",
        stateMutability: "view",
        inputs: [],
        outputs: [
          {
            type: "string"
          }
        ]
      },
      {
        type: "function",
        name: "totalSupply",
        stateMutability: "view",
        inputs: [],
        outputs: [
          {
            type: "uint256"
          }
        ]
      },
      {
        type: "function",
        name: "transfer",
        stateMutability: "nonpayable",
        inputs: [
          {
            name: "recipient",
            type: "address"
          },
          {
            name: "amount",
            type: "uint256"
          }
        ],
        outputs: [
          {
            type: "bool"
          }
        ]
      },
      {
        type: "function",
        name: "transferFrom",
        stateMutability: "nonpayable",
        inputs: [
          {
            name: "sender",
            type: "address"
          },
          {
            name: "recipient",
            type: "address"
          },
          {
            name: "amount",
            type: "uint256"
          }
        ],
        outputs: [
          {
            type: "bool"
          }
        ]
      }
    ];
  }
});

// node_modules/@anon-rpc/harness-core/dist/call-queue.js
var CallQueue;
var init_call_queue = __esm({
  "node_modules/@anon-rpc/harness-core/dist/call-queue.js"() {
    CallQueue = class {
      #items = [];
      #waiters = [];
      /** Resolve with the next item; an abort withdraws this taker (§8: it must not consume an item). */
      take(signal) {
        const next = this.#items.shift();
        if (next !== void 0) return Promise.resolve(next);
        return new Promise((resolve, reject) => {
          const waiter = {
            resolve: (item) => {
              signal?.removeEventListener("abort", onAbort);
              resolve(item);
            },
            reject: (e) => {
              signal?.removeEventListener("abort", onAbort);
              reject(e);
            }
          };
          const onAbort = () => {
            const i = this.#waiters.indexOf(waiter);
            if (i >= 0) this.#waiters.splice(i, 1);
            reject(signal?.reason ?? new DOMException("aborted", "AbortError"));
          };
          if (signal?.aborted) return onAbort();
          signal?.addEventListener("abort", onAbort, { once: true });
          this.#waiters.push(waiter);
        });
      }
      /** Deliver to the oldest pending take(), else queue in arrival order. */
      push(item) {
        const waiter = this.#waiters.shift();
        if (waiter) waiter.resolve(item);
        else this.#items.push(item);
      }
      /** Return an item to the FRONT of the queue (it was taken but not delivered). */
      pushFront(item) {
        const waiter = this.#waiters.shift();
        if (waiter) waiter.resolve(item);
        else this.#items.unshift(item);
      }
      /** Withdraw a not-yet-delivered item. Returns false if it was already taken (or never queued). */
      remove(item) {
        const i = this.#items.indexOf(item);
        if (i < 0) return false;
        this.#items.splice(i, 1);
        return true;
      }
      /**
       * Drop every queued item and reject every pending take(), returning the
       * items that were dropped.
       *
       * The return value is the point. An item in this queue is a call some caller
       * is still awaiting, and the queue holds no way to reject it — so emptying
       * the array is not cancelling the call, it is losing it, and the caller waits
       * forever. Whoever queued the items has to settle them; returning them here
       * is what makes forgetting to hard.
       */
      rejectAll(err) {
        const dropped = this.#items.splice(0);
        for (const w of this.#waiters.splice(0)) w.reject(err);
        return dropped;
      }
    };
  }
});

// node_modules/@anon-rpc/harness-core/dist/log-queue.js
var LogQueue;
var init_log_queue = __esm({
  "node_modules/@anon-rpc/harness-core/dist/log-queue.js"() {
    LogQueue = class {
      #items = [];
      #waiters = [];
      #closed;
      #max;
      constructor(max) {
        this.#max = max;
      }
      /** How many entries have been dropped for want of room. */
      dropped = 0;
      /**
       * Resolve with the next entry, waiting if there is none.
       *
       * §13.1: an abort withdraws the caller without consuming an entry, and a
       * closed queue still yields whatever it retained before rejecting — the
       * entries explaining a failure are the ones most worth reading.
       */
      take(signal) {
        const next = this.#items.shift();
        if (next !== void 0) return Promise.resolve(next);
        if (this.#closed) return Promise.reject(this.#closed.err);
        return new Promise((resolve, reject) => {
          const waiter = {
            resolve: (item) => {
              signal?.removeEventListener("abort", onAbort);
              resolve(item);
            },
            reject: (e) => {
              signal?.removeEventListener("abort", onAbort);
              reject(e);
            }
          };
          const onAbort = () => {
            const i = this.#waiters.indexOf(waiter);
            if (i >= 0) this.#waiters.splice(i, 1);
            reject(signal?.reason ?? new DOMException("aborted", "AbortError"));
          };
          if (signal?.aborted) return onAbort();
          signal?.addEventListener("abort", onAbort, { once: true });
          this.#waiters.push(waiter);
        });
      }
      /** Deliver to the oldest waiter, else retain — dropping the oldest if full. */
      push(item) {
        if (this.#closed) return;
        const waiter = this.#waiters.shift();
        if (waiter) return waiter.resolve(item);
        this.#items.push(item);
        while (this.#items.length > this.#max) {
          this.#items.shift();
          this.dropped++;
        }
      }
      /**
       * Stop accepting entries; retained ones stay readable, and `take()` rejects
       * once they run out.
       *
       * Pending waiters reject immediately: there is nothing left to wait for, and
       * a host looping on `acceptLog()` would otherwise hang on a dead worker.
       */
      close(err) {
        if (this.#closed) return;
        this.#closed = { err };
        for (const w of this.#waiters.splice(0)) w.reject(err);
      }
      /** Retained, undelivered entries. */
      get size() {
        return this.#items.length;
      }
    };
  }
});

// node_modules/@anon-rpc/harness-core/node_modules/@noble/hashes/esm/_u64.js
function fromBig2(n, le = false) {
  if (le)
    return { h: Number(n & U32_MASK642), l: Number(n >> _32n2 & U32_MASK642) };
  return { h: Number(n >> _32n2 & U32_MASK642) | 0, l: Number(n & U32_MASK642) | 0 };
}
function split2(lst, le = false) {
  const len = lst.length;
  let Ah = new Uint32Array(len);
  let Al = new Uint32Array(len);
  for (let i = 0; i < len; i++) {
    const { h, l } = fromBig2(lst[i], le);
    [Ah[i], Al[i]] = [h, l];
  }
  return [Ah, Al];
}
var U32_MASK642, _32n2, rotlSH2, rotlSL2, rotlBH2, rotlBL2;
var init_u642 = __esm({
  "node_modules/@anon-rpc/harness-core/node_modules/@noble/hashes/esm/_u64.js"() {
    U32_MASK642 = /* @__PURE__ */ BigInt(2 ** 32 - 1);
    _32n2 = /* @__PURE__ */ BigInt(32);
    rotlSH2 = (h, l, s) => h << s | l >>> 32 - s;
    rotlSL2 = (h, l, s) => l << s | h >>> 32 - s;
    rotlBH2 = (h, l, s) => l << s - 32 | h >>> 64 - s;
    rotlBL2 = (h, l, s) => h << s - 32 | l >>> 64 - s;
  }
});

// node_modules/@anon-rpc/harness-core/node_modules/@noble/hashes/esm/utils.js
function isBytes2(a) {
  return a instanceof Uint8Array || ArrayBuffer.isView(a) && a.constructor.name === "Uint8Array";
}
function anumber2(n) {
  if (!Number.isSafeInteger(n) || n < 0)
    throw new Error("positive integer expected, got " + n);
}
function abytes2(b, ...lengths) {
  if (!isBytes2(b))
    throw new Error("Uint8Array expected");
  if (lengths.length > 0 && !lengths.includes(b.length))
    throw new Error("Uint8Array expected of length " + lengths + ", got length=" + b.length);
}
function aexists2(instance, checkFinished = true) {
  if (instance.destroyed)
    throw new Error("Hash instance has been destroyed");
  if (checkFinished && instance.finished)
    throw new Error("Hash#digest() has already been called");
}
function aoutput2(out2, instance) {
  abytes2(out2);
  const min = instance.outputLen;
  if (out2.length < min) {
    throw new Error("digestInto() expects output buffer of length at least " + min);
  }
}
function u322(arr) {
  return new Uint32Array(arr.buffer, arr.byteOffset, Math.floor(arr.byteLength / 4));
}
function clean2(...arrays) {
  for (let i = 0; i < arrays.length; i++) {
    arrays[i].fill(0);
  }
}
function byteSwap2(word2) {
  return word2 << 24 & 4278190080 | word2 << 8 & 16711680 | word2 >>> 8 & 65280 | word2 >>> 24 & 255;
}
function byteSwap322(arr) {
  for (let i = 0; i < arr.length; i++) {
    arr[i] = byteSwap2(arr[i]);
  }
  return arr;
}
function utf8ToBytes2(str) {
  if (typeof str !== "string")
    throw new Error("string expected");
  return new Uint8Array(new TextEncoder().encode(str));
}
function toBytes3(data) {
  if (typeof data === "string")
    data = utf8ToBytes2(data);
  abytes2(data);
  return data;
}
function createHasher2(hashCons) {
  const hashC = (msg) => hashCons().update(toBytes3(msg)).digest();
  const tmp = hashCons();
  hashC.outputLen = tmp.outputLen;
  hashC.blockLen = tmp.blockLen;
  hashC.create = () => hashCons();
  return hashC;
}
var isLE2, swap32IfBE2, Hash2;
var init_utils2 = __esm({
  "node_modules/@anon-rpc/harness-core/node_modules/@noble/hashes/esm/utils.js"() {
    isLE2 = /* @__PURE__ */ (() => new Uint8Array(new Uint32Array([287454020]).buffer)[0] === 68)();
    swap32IfBE2 = isLE2 ? (u) => u : byteSwap322;
    Hash2 = class {
    };
  }
});

// node_modules/@anon-rpc/harness-core/node_modules/@noble/hashes/esm/sha3.js
function keccakP2(s, rounds = 24) {
  const B = new Uint32Array(5 * 2);
  for (let round = 24 - rounds; round < 24; round++) {
    for (let x = 0; x < 10; x++)
      B[x] = s[x] ^ s[x + 10] ^ s[x + 20] ^ s[x + 30] ^ s[x + 40];
    for (let x = 0; x < 10; x += 2) {
      const idx1 = (x + 8) % 10;
      const idx0 = (x + 2) % 10;
      const B0 = B[idx0];
      const B1 = B[idx0 + 1];
      const Th = rotlH2(B0, B1, 1) ^ B[idx1];
      const Tl = rotlL2(B0, B1, 1) ^ B[idx1 + 1];
      for (let y = 0; y < 50; y += 10) {
        s[x + y] ^= Th;
        s[x + y + 1] ^= Tl;
      }
    }
    let curH = s[2];
    let curL = s[3];
    for (let t = 0; t < 24; t++) {
      const shift = SHA3_ROTL2[t];
      const Th = rotlH2(curH, curL, shift);
      const Tl = rotlL2(curH, curL, shift);
      const PI = SHA3_PI2[t];
      curH = s[PI];
      curL = s[PI + 1];
      s[PI] = Th;
      s[PI + 1] = Tl;
    }
    for (let y = 0; y < 50; y += 10) {
      for (let x = 0; x < 10; x++)
        B[x] = s[y + x];
      for (let x = 0; x < 10; x++)
        s[y + x] ^= ~B[(x + 2) % 10] & B[(x + 4) % 10];
    }
    s[0] ^= SHA3_IOTA_H2[round];
    s[1] ^= SHA3_IOTA_L2[round];
  }
  clean2(B);
}
var _0n2, _1n2, _2n2, _7n2, _256n2, _0x71n2, SHA3_PI2, SHA3_ROTL2, _SHA3_IOTA2, IOTAS2, SHA3_IOTA_H2, SHA3_IOTA_L2, rotlH2, rotlL2, Keccak2, gen2, keccak_2562;
var init_sha32 = __esm({
  "node_modules/@anon-rpc/harness-core/node_modules/@noble/hashes/esm/sha3.js"() {
    init_u642();
    init_utils2();
    _0n2 = BigInt(0);
    _1n2 = BigInt(1);
    _2n2 = BigInt(2);
    _7n2 = BigInt(7);
    _256n2 = BigInt(256);
    _0x71n2 = BigInt(113);
    SHA3_PI2 = [];
    SHA3_ROTL2 = [];
    _SHA3_IOTA2 = [];
    for (let round = 0, R2 = _1n2, x = 1, y = 0; round < 24; round++) {
      [x, y] = [y, (2 * x + 3 * y) % 5];
      SHA3_PI2.push(2 * (5 * y + x));
      SHA3_ROTL2.push((round + 1) * (round + 2) / 2 % 64);
      let t = _0n2;
      for (let j = 0; j < 7; j++) {
        R2 = (R2 << _1n2 ^ (R2 >> _7n2) * _0x71n2) % _256n2;
        if (R2 & _2n2)
          t ^= _1n2 << (_1n2 << /* @__PURE__ */ BigInt(j)) - _1n2;
      }
      _SHA3_IOTA2.push(t);
    }
    IOTAS2 = split2(_SHA3_IOTA2, true);
    SHA3_IOTA_H2 = IOTAS2[0];
    SHA3_IOTA_L2 = IOTAS2[1];
    rotlH2 = (h, l, s) => s > 32 ? rotlBH2(h, l, s) : rotlSH2(h, l, s);
    rotlL2 = (h, l, s) => s > 32 ? rotlBL2(h, l, s) : rotlSL2(h, l, s);
    Keccak2 = class _Keccak extends Hash2 {
      // NOTE: we accept arguments in bytes instead of bits here.
      constructor(blockLen, suffix, outputLen, enableXOF = false, rounds = 24) {
        super();
        this.pos = 0;
        this.posOut = 0;
        this.finished = false;
        this.destroyed = false;
        this.enableXOF = false;
        this.blockLen = blockLen;
        this.suffix = suffix;
        this.outputLen = outputLen;
        this.enableXOF = enableXOF;
        this.rounds = rounds;
        anumber2(outputLen);
        if (!(0 < blockLen && blockLen < 200))
          throw new Error("only keccak-f1600 function is supported");
        this.state = new Uint8Array(200);
        this.state32 = u322(this.state);
      }
      clone() {
        return this._cloneInto();
      }
      keccak() {
        swap32IfBE2(this.state32);
        keccakP2(this.state32, this.rounds);
        swap32IfBE2(this.state32);
        this.posOut = 0;
        this.pos = 0;
      }
      update(data) {
        aexists2(this);
        data = toBytes3(data);
        abytes2(data);
        const { blockLen, state } = this;
        const len = data.length;
        for (let pos = 0; pos < len; ) {
          const take = Math.min(blockLen - this.pos, len - pos);
          for (let i = 0; i < take; i++)
            state[this.pos++] ^= data[pos++];
          if (this.pos === blockLen)
            this.keccak();
        }
        return this;
      }
      finish() {
        if (this.finished)
          return;
        this.finished = true;
        const { state, suffix, pos, blockLen } = this;
        state[pos] ^= suffix;
        if ((suffix & 128) !== 0 && pos === blockLen - 1)
          this.keccak();
        state[blockLen - 1] ^= 128;
        this.keccak();
      }
      writeInto(out2) {
        aexists2(this, false);
        abytes2(out2);
        this.finish();
        const bufferOut = this.state;
        const { blockLen } = this;
        for (let pos = 0, len = out2.length; pos < len; ) {
          if (this.posOut >= blockLen)
            this.keccak();
          const take = Math.min(blockLen - this.posOut, len - pos);
          out2.set(bufferOut.subarray(this.posOut, this.posOut + take), pos);
          this.posOut += take;
          pos += take;
        }
        return out2;
      }
      xofInto(out2) {
        if (!this.enableXOF)
          throw new Error("XOF is not possible for this instance");
        return this.writeInto(out2);
      }
      xof(bytes) {
        anumber2(bytes);
        return this.xofInto(new Uint8Array(bytes));
      }
      digestInto(out2) {
        aoutput2(out2, this);
        if (this.finished)
          throw new Error("digest() was already called");
        this.writeInto(out2);
        this.destroy();
        return out2;
      }
      digest() {
        return this.digestInto(new Uint8Array(this.outputLen));
      }
      destroy() {
        this.destroyed = true;
        clean2(this.state);
      }
      _cloneInto(to) {
        const { blockLen, suffix, outputLen, rounds, enableXOF } = this;
        to || (to = new _Keccak(blockLen, suffix, outputLen, enableXOF, rounds));
        to.state32.set(this.state32);
        to.pos = this.pos;
        to.posOut = this.posOut;
        to.finished = this.finished;
        to.rounds = rounds;
        to.suffix = suffix;
        to.outputLen = outputLen;
        to.enableXOF = enableXOF;
        to.destroyed = this.destroyed;
        return to;
      }
    };
    gen2 = (suffix, blockLen, outputLen) => createHasher2(() => new Keccak2(blockLen, suffix, outputLen));
    keccak_2562 = /* @__PURE__ */ (() => gen2(1, 136, 256 / 8))();
  }
});

// node_modules/@anon-rpc/harness-core/dist/hex.js
function toHex2(bytes) {
  let s = "";
  for (const b of bytes) s += b.toString(16).padStart(2, "0");
  return s;
}
var init_hex = __esm({
  "node_modules/@anon-rpc/harness-core/dist/hex.js"() {
  }
});

// node_modules/@anon-rpc/harness-core/dist/specifier.js
function selector(sig) {
  return "0x" + toHex2(keccak_2562(new TextEncoder().encode(sig))).slice(0, 8);
}
function hexToBytes2(hex) {
  const h = hex.startsWith("0x") ? hex.slice(2) : hex;
  const out2 = new Uint8Array(h.length / 2);
  for (let i = 0; i < out2.length; i++) out2[i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
  return out2;
}
async function readSpecifier(provider, address) {
  const hashRet = await ethCall(provider, address, SEL_WORKER_HASH);
  const resolversRet = await ethCall(provider, address, SEL_WORKER_RESOLVERS);
  return {
    workerHash: "0x" + decodeBytes32(hashRet),
    resolvers: decodeStringArray(resolversRet)
  };
}
async function ethCall(provider, to, data) {
  const res = await provider.request({
    method: "eth_call",
    params: [{ to, data }, "latest"]
  });
  if (typeof res !== "string") throw new Error("eth_call did not return data");
  return hexToBytes2(res);
}
async function readBodyCapped(resp, cap) {
  const overflow = () => new Error(`body exceeds the ${cap}-byte bundle cap`);
  if (!resp.body) {
    const buf = new Uint8Array(await resp.arrayBuffer());
    if (buf.byteLength > cap) throw overflow();
    return buf;
  }
  const reader = resp.body.getReader();
  const chunks = [];
  let total = 0;
  for (; ; ) {
    const { value, done } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > cap) {
      await reader.cancel().catch(() => {
      });
      throw overflow();
    }
    chunks.push(value);
  }
  const out2 = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) {
    out2.set(c, off);
    off += c.byteLength;
  }
  return out2;
}
async function fetchAndVerifyBundle(spec, maxBytes = MAX_BUNDLE_BYTES, kinds = []) {
  const errors = [];
  for (const url of spec.resolvers) {
    let bytes;
    try {
      if (/^(https?|blob):/.test(url)) {
        const resp = await fetch(url);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        bytes = await readBodyCapped(resp, maxBytes);
      } else {
        const kind = kinds.find((k) => k.matches(url));
        if (!kind) {
          errors.push(`${url}: unrecognized resolver kind (ignored)`);
          continue;
        }
        bytes = await kind.fetch(url, maxBytes);
      }
    } catch (e) {
      errors.push(`${url}: ${e.message}`);
      continue;
    }
    const got = "0x" + toHex2(keccak_2562(bytes));
    if (got !== spec.workerHash) {
      errors.push(`${url}: hash mismatch (got ${got}, want ${spec.workerHash})`);
      continue;
    }
    return bytes;
  }
  throw new Error(`no resolver yielded bytes matching workerHash: ${errors.join("; ")}`);
}
function decodeBytes32(ret) {
  if (ret.length < 32) throw new Error("short bytes32 return");
  return toHex2(ret.slice(0, 32));
}
function word(ret, at) {
  if (at < 0 || at + 32 > ret.length) throw new Error("ABI decode: word out of bounds");
  let n = 0;
  for (let i = at; i < at + 32; i++) {
    n = n * 256 + ret[i];
    if (n > Number.MAX_SAFE_INTEGER) throw new Error("ABI decode: word too large");
  }
  return n;
}
function decodeStringArray(ret) {
  const base = word(ret, 0);
  const len = word(ret, base);
  if (base + 32 + len * 32 > ret.length) throw new Error("ABI decode: array length out of bounds");
  const out2 = [];
  const dec = new TextDecoder();
  for (let i = 0; i < len; i++) {
    const elemOff = base + 32 + word(ret, base + 32 + i * 32);
    const strLen = word(ret, elemOff);
    if (elemOff + 32 + strLen > ret.length) throw new Error("ABI decode: string out of bounds");
    out2.push(dec.decode(ret.slice(elemOff + 32, elemOff + 32 + strLen)));
  }
  return out2;
}
var SEL_WORKER_HASH, SEL_WORKER_RESOLVERS, MAX_BUNDLE_BYTES;
var init_specifier = __esm({
  "node_modules/@anon-rpc/harness-core/dist/specifier.js"() {
    init_sha32();
    init_hex();
    SEL_WORKER_HASH = selector("workerHash()");
    SEL_WORKER_RESOLVERS = selector("workerResolvers()");
    MAX_BUNDLE_BYTES = 64 * 1024 * 1024;
  }
});

// node_modules/@anon-rpc/harness-core/dist/plain-value.js
function copyPlainValue(value, path = "value") {
  return copy(value, path, /* @__PURE__ */ new Set());
}
function copy(v, path, ancestors) {
  switch (typeof v) {
    case "undefined":
    case "boolean":
    case "number":
    // delivered exactly: NaN, ±Infinity and -0 included
    case "string":
      return v;
    case "object":
      break;
    default:
      throw notPlain(path, typeof v);
  }
  if (v === null) return null;
  if (v instanceof Uint8Array) {
    if (isDetached(v.buffer)) throw notPlain(path, "a Uint8Array over a detached buffer");
    return new Uint8Array(v);
  }
  if (v instanceof ArrayBuffer) {
    if (isDetached(v)) throw notPlain(path, "a detached ArrayBuffer");
    return v.slice(0);
  }
  if (ancestors.has(v)) throw notPlain(path, "a value that contains itself");
  ancestors.add(v);
  try {
    if (Array.isArray(v)) {
      const out22 = new Array(v.length);
      for (let i = 0; i < v.length; i++) out22[i] = copy(v[i], `${path}[${i}]`, ancestors);
      return out22;
    }
    const proto = Object.getPrototypeOf(v);
    if (proto !== Object.prototype && proto !== null) {
      throw notPlain(path, describe(v));
    }
    const out2 = {};
    for (const key of Object.keys(v)) {
      Object.defineProperty(out2, key, {
        value: copy(v[key], `${path}.${key}`, ancestors),
        writable: true,
        enumerable: true,
        configurable: true
      });
    }
    return out2;
  } finally {
    ancestors.delete(v);
  }
}
function isDetached(b) {
  return b.detached === true;
}
function describe(v) {
  const name = v.constructor?.name;
  return typeof name === "string" && name ? `a ${name}` : "an object with a non-plain prototype";
}
function notPlain(path, what) {
  return new TypeError(`${path} is not a plain value (\xA79.4): got ${what}`);
}
var init_plain_value = __esm({
  "node_modules/@anon-rpc/harness-core/dist/plain-value.js"() {
  }
});

// node_modules/@anon-rpc/harness-core/dist/forwarded.js
function copyForwardedEntries(init) {
  if (!init) return void 0;
  let out2;
  for (const key of Object.keys(init)) {
    if (MODELED_INIT_KEYS.has(key)) continue;
    out2 ??= {};
    Object.defineProperty(out2, key, {
      value: copyPlainValue(init[key], `init.${key}`),
      writable: true,
      enumerable: true,
      configurable: true
    });
  }
  return out2;
}
var MODELED_INIT_KEYS;
var init_forwarded = __esm({
  "node_modules/@anon-rpc/harness-core/dist/forwarded.js"() {
    init_plain_value();
    MODELED_INIT_KEYS = /* @__PURE__ */ new Set(["method", "headers", "body", "redirect", "signal"]);
  }
});

// node_modules/@anon-rpc/harness-core/dist/errors.js
function serializeError(err) {
  if (err instanceof Error) {
    const code = err.code;
    return {
      name: err.name,
      message: err.message,
      ...typeof code === "string" ? { code } : {}
    };
  }
  return { name: "Error", message: String(err) };
}
var RpcError;
var init_errors = __esm({
  "node_modules/@anon-rpc/harness-core/dist/errors.js"() {
    RpcError = class extends Error {
      code;
      constructor(e) {
        super(e.message);
        this.name = e.name;
        this.code = e.code;
      }
    };
  }
});

// node_modules/@anon-rpc/harness-core/dist/index.js
var init_dist = __esm({
  "node_modules/@anon-rpc/harness-core/dist/index.js"() {
    init_call_queue();
    init_log_queue();
    init_specifier();
    init_plain_value();
    init_forwarded();
    init_errors();
  }
});

// node_modules/@kpstreams/core/dist/address.js
function parseAddress(s) {
  const malformed = () => new Error(`address: malformed (expected <ip>:<port>:<certhash> or [ipv6]:<port>:<certhash>): ${s}`);
  let ip;
  let rest;
  if (s.startsWith("[")) {
    const end = s.indexOf("]");
    if (end < 0 || s[end + 1] !== ":")
      throw malformed();
    ip = s.slice(1, end);
    rest = s.slice(end + 2);
  } else {
    const i = s.indexOf(":");
    if (i < 0)
      throw malformed();
    ip = s.slice(0, i);
    rest = s.slice(i + 1);
  }
  const j = rest.indexOf(":");
  if (j < 0)
    throw malformed();
  const portStr = rest.slice(0, j);
  const certhash = rest.slice(j + 1);
  if (!/^\d+$/.test(portStr))
    throw malformed();
  const port = Number(portStr);
  if (port < 1 || port > 65535)
    throw new Error("address: port out of range");
  if (!ip || !certhash)
    throw malformed();
  return { ip, port, certhash };
}
function formatAddress(addr) {
  const host = addr.ip.includes(":") ? `[${addr.ip}]` : addr.ip;
  return `${host}:${addr.port}:${addr.certhash}`;
}
var init_address2 = __esm({
  "node_modules/@kpstreams/core/dist/address.js"() {
  }
});

// node_modules/@kpstreams/core/dist/certhash.js
function decodeCerthash(s) {
  if (!s.startsWith(MULTIBASE_BASE64URL_NOPAD)) {
    throw new Error(`certhash: expected multibase prefix '${MULTIBASE_BASE64URL_NOPAD}', got '${s[0] ?? ""}'`);
  }
  const bytes = base64urlDecode(s.slice(1));
  if (bytes.length !== 2 + MULTIHASH_SHA256_LEN) {
    throw new Error(`certhash: expected ${2 + MULTIHASH_SHA256_LEN} bytes, got ${bytes.length}`);
  }
  if (bytes[0] !== MULTIHASH_SHA256_CODE || bytes[1] !== MULTIHASH_SHA256_LEN) {
    throw new Error(`certhash: not a sha2-256 multihash (prefix ${bytes[0].toString(16)} ${bytes[1].toString(16)})`);
  }
  return bytes.slice(2);
}
function digestToSdpFingerprint(digest) {
  return Array.from(digest, (b) => b.toString(16).padStart(2, "0").toUpperCase()).join(":");
}
function base64urlDecode(s) {
  const pad3 = (4 - s.length % 4) % 4;
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat(pad3);
  const bin = atob(b64);
  const out2 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++)
    out2[i] = bin.charCodeAt(i);
  return out2;
}
var MULTIBASE_BASE64URL_NOPAD, MULTIHASH_SHA256_CODE, MULTIHASH_SHA256_LEN;
var init_certhash = __esm({
  "node_modules/@kpstreams/core/dist/certhash.js"() {
    MULTIBASE_BASE64URL_NOPAD = "u";
    MULTIHASH_SHA256_CODE = 18;
    MULTIHASH_SHA256_LEN = 32;
  }
});

// node_modules/@kpstreams/core/dist/errors.js
function streamError(reason) {
  const e = new Error(reason.message ?? `kps: stream ${reason.code ?? "reset"}`);
  e.code = reason.code;
  return e;
}
function reasonFrom(x) {
  if (x == null)
    return void 0;
  if (typeof x === "object" && ("code" in x || "message" in x))
    return x;
  return { message: String(x?.message ?? x) };
}
var init_errors2 = __esm({
  "node_modules/@kpstreams/core/dist/errors.js"() {
  }
});

// node_modules/@kpstreams/core/dist/index.js
var init_dist2 = __esm({
  "node_modules/@kpstreams/core/dist/index.js"() {
    init_address2();
    init_certhash();
  }
});

// node_modules/@kpstreams/core/dist/framing.js
function codeToNum(code) {
  return code ? CODE_TO_NUM[code] ?? 0 : 0;
}
function numToCode(n) {
  return n === 0 ? void 0 : NUM_TO_CODE[n] ?? "internal-error";
}
function encodeData(payload) {
  const out2 = new Uint8Array(1 + payload.length);
  out2[0] = FRAME_DATA;
  out2.set(payload, 1);
  return out2;
}
function encodeFin() {
  return new Uint8Array([FRAME_FIN]);
}
function encodeCode(type, code) {
  const out2 = new Uint8Array(5);
  out2[0] = type;
  new DataView(out2.buffer).setUint32(1, code >>> 0, false);
  return out2;
}
function encodeMaxStreamData(value) {
  const out2 = new Uint8Array(9);
  out2[0] = FRAME_MAX_STREAM_DATA;
  new DataView(out2.buffer).setBigUint64(1, value, false);
  return out2;
}
function parseFrame(data) {
  if (data.length === 0)
    throw new ProtocolViolation("empty data-channel message");
  if (data.length > MAX_WEBRTC_FRAME_SIZE) {
    throw new ProtocolViolation(`frame exceeds ${MAX_WEBRTC_FRAME_SIZE} bytes (${data.length})`);
  }
  const type = data[0];
  const payload = data.subarray(1);
  const view = () => new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  switch (type) {
    case FRAME_DATA:
      if (payload.length === 0)
        throw new ProtocolViolation("empty DATA frame");
      return { type: "data", payload };
    case FRAME_FIN:
      if (payload.length !== 0)
        throw new ProtocolViolation("FIN with payload");
      return { type: "fin" };
    case FRAME_RESET:
      if (payload.length !== 4)
        throw new ProtocolViolation("RESET payload must be 4 bytes");
      return { type: "reset", code: view().getUint32(0, false) };
    case FRAME_STOP_SENDING:
      if (payload.length !== 4)
        throw new ProtocolViolation("STOP_SENDING payload must be 4 bytes");
      return { type: "stop-sending", code: view().getUint32(0, false) };
    case FRAME_MAX_STREAM_DATA: {
      if (payload.length !== 8)
        throw new ProtocolViolation("MAX_STREAM_DATA payload must be 8 bytes");
      const value = view().getBigUint64(0, false);
      if (value > MAX_OFFSET)
        throw new ProtocolViolation("MAX_STREAM_DATA above MAX_OFFSET");
      return { type: "max-stream-data", value };
    }
    default:
      throw new ProtocolViolation(`unknown frame type 0x${type.toString(16)}`);
  }
}
var FRAME_DATA, FRAME_FIN, FRAME_RESET, FRAME_STOP_SENDING, FRAME_MAX_STREAM_DATA, MAX_WEBRTC_FRAME_SIZE, MAX_FRAME_PAYLOAD, MAX_OFFSET, ProtocolViolation, CODE_TO_NUM, NUM_TO_CODE;
var init_framing = __esm({
  "node_modules/@kpstreams/core/dist/framing.js"() {
    FRAME_DATA = 0;
    FRAME_FIN = 1;
    FRAME_RESET = 2;
    FRAME_STOP_SENDING = 3;
    FRAME_MAX_STREAM_DATA = 4;
    MAX_WEBRTC_FRAME_SIZE = 16384;
    MAX_FRAME_PAYLOAD = MAX_WEBRTC_FRAME_SIZE - 1;
    MAX_OFFSET = (1n << 62n) - 1n;
    ProtocolViolation = class extends Error {
    };
    CODE_TO_NUM = {
      cancelled: 1,
      closed: 2,
      reset: 3,
      timeout: 4,
      "network-error": 5,
      "protocol-error": 6,
      unsupported: 7,
      "too-large": 8,
      "queue-full": 9,
      "permission-denied": 10,
      "internal-error": 11
    };
    NUM_TO_CODE = Object.fromEntries(Object.entries(CODE_TO_NUM).map(([k, v]) => [v, k]));
  }
});

// node_modules/@kpstreams/core/dist/control.js
function encodeConnClose(code) {
  const out2 = new Uint8Array(5);
  out2[0] = CTRL_CONNECTION_CLOSE;
  new DataView(out2.buffer).setUint32(1, codeToNum(code), false);
  return out2;
}
function encodeHello(limits, version3 = WIRE_VERSION) {
  const out2 = new Uint8Array(26);
  const v = new DataView(out2.buffer);
  out2[0] = CTRL_HELLO;
  out2[1] = version3;
  v.setBigUint64(2, limits.initialMaxStreamData, false);
  v.setBigUint64(10, limits.initialMaxData, false);
  v.setBigUint64(18, limits.initialMaxStreams, false);
  return out2;
}
function encodeMaxData(value) {
  const out2 = new Uint8Array(9);
  out2[0] = CTRL_MAX_DATA;
  new DataView(out2.buffer).setBigUint64(1, value, false);
  return out2;
}
function encodeMaxStreams(value) {
  const out2 = new Uint8Array(9);
  out2[0] = CTRL_MAX_STREAMS;
  new DataView(out2.buffer).setBigUint64(1, value, false);
  return out2;
}
function decodeControl(data) {
  if (data.length === 0)
    throw new ProtocolViolation("empty control message");
  const type = data[0];
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  switch (type) {
    case CTRL_CONNECTION_CLOSE:
      if (data.length !== 5)
        throw new ProtocolViolation("CONNECTION_CLOSE must be 5 bytes");
      return { t: "close", code: view.getUint32(1, false) };
    case CTRL_HELLO: {
      if (data.length !== 26)
        throw new ProtocolViolation("HELLO must be 26 bytes");
      const limits = {
        initialMaxStreamData: view.getBigUint64(2, false),
        initialMaxData: view.getBigUint64(10, false),
        initialMaxStreams: view.getBigUint64(18, false)
      };
      for (const v of Object.values(limits)) {
        if (v > MAX_OFFSET)
          throw new ProtocolViolation("HELLO credit above MAX_OFFSET");
      }
      return { t: "hello", version: data[1], limits };
    }
    case CTRL_MAX_DATA: {
      if (data.length !== 9)
        throw new ProtocolViolation("MAX_DATA must be 9 bytes");
      const value = view.getBigUint64(1, false);
      if (value > MAX_OFFSET)
        throw new ProtocolViolation("MAX_DATA above MAX_OFFSET");
      return { t: "max-data", value };
    }
    case CTRL_MAX_STREAMS: {
      if (data.length !== 9)
        throw new ProtocolViolation("MAX_STREAMS must be 9 bytes");
      const value = view.getBigUint64(1, false);
      if (value > MAX_OFFSET)
        throw new ProtocolViolation("MAX_STREAMS above MAX_OFFSET");
      return { t: "max-streams", value };
    }
    default:
      throw new ProtocolViolation(`unknown control message type 0x${type.toString(16)}`);
  }
}
var CTRL_CONNECTION_CLOSE, CTRL_HELLO, CTRL_MAX_DATA, CTRL_MAX_STREAMS, WIRE_VERSION;
var init_control = __esm({
  "node_modules/@kpstreams/core/dist/control.js"() {
    init_framing();
    CTRL_CONNECTION_CLOSE = 0;
    CTRL_HELLO = 1;
    CTRL_MAX_DATA = 2;
    CTRL_MAX_STREAMS = 3;
    WIRE_VERSION = 1;
  }
});

// node_modules/@kpstreams/core/dist/flow.js
function resolveLimits(partial) {
  return {
    initialMaxStreamData: partial?.initialMaxStreamData ?? DEFAULT_INITIAL_MAX_STREAM_DATA,
    initialMaxData: partial?.initialMaxData ?? DEFAULT_INITIAL_MAX_DATA,
    initialMaxStreams: partial?.initialMaxStreams ?? DEFAULT_INITIAL_MAX_STREAMS
  };
}
function saturate(v) {
  return v > MAX_OFFSET ? MAX_OFFSET : v;
}
var DEFAULT_INITIAL_MAX_STREAM_DATA, DEFAULT_INITIAL_MAX_DATA, DEFAULT_INITIAL_MAX_STREAMS, Wakeable, ConnFlow, StreamFlow;
var init_flow = __esm({
  "node_modules/@kpstreams/core/dist/flow.js"() {
    init_framing();
    DEFAULT_INITIAL_MAX_STREAM_DATA = 1n << 20n;
    DEFAULT_INITIAL_MAX_DATA = 8n << 20n;
    DEFAULT_INITIAL_MAX_STREAMS = 100n;
    Wakeable = class {
      #resolvers = [];
      wake() {
        const rs = this.#resolvers;
        this.#resolvers = [];
        for (const r of rs)
          r();
      }
      wait(signal) {
        return new Promise((resolve, reject) => {
          if (signal) {
            const onAbort = () => reject(new Error("kps: aborted"));
            signal.addEventListener("abort", onAbort, { once: true });
            this.#resolvers.push(() => {
              signal.removeEventListener("abort", onAbort);
              resolve();
            });
          } else {
            this.#resolvers.push(resolve);
          }
        });
      }
    };
    ConnFlow = class {
      // ---- our receive policy (what we grant the peer) ----
      local;
      // ---- sender side (peer-granted, all zero until the peer's HELLO) ----
      #peerMaxStreamDataInitial = 0n;
      // seeds each new StreamFlow's send window
      #peerMaxData = 0n;
      #peerMaxStreams = 0n;
      #connSent = 0n;
      #connReserved = 0n;
      #streamsOpened = 0n;
      #streamsReserved = 0n;
      // ---- receiver side ----
      #localMaxData;
      // enforcement limit (advances at commit-to-send)
      #connReceived = 0n;
      #connConsumed = 0n;
      #connAdvertisedAt = 0n;
      // connConsumed value at the last advertisement
      #peerOpenedStreams = 0n;
      #peerRetiredStreams = 0n;
      #advertisedMaxStreams;
      #credit = new Wakeable();
      #failure = null;
      #sink;
      constructor(local, sink) {
        this.local = local;
        this.#localMaxData = local.initialMaxData;
        this.#advertisedMaxStreams = local.initialMaxStreams;
        this.#sink = sink;
      }
      /** The peer's HELLO: seed every send-side limit. */
      onPeerHello(limits) {
        this.#peerMaxStreamDataInitial = limits.initialMaxStreamData;
        this.#peerMaxData = limits.initialMaxData;
        this.#peerMaxStreams = limits.initialMaxStreams;
        this.#credit.wake();
      }
      /** Peer raised the connection data limit (MAX_DATA). Decreases are ignored. */
      onPeerMaxData(value) {
        if (value > this.#peerMaxData) {
          this.#peerMaxData = value;
          this.#credit.wake();
        }
      }
      /** Peer raised the stream-count limit (MAX_STREAMS). Decreases are ignored. */
      onPeerMaxStreams(value) {
        if (value > this.#peerMaxStreams) {
          this.#peerMaxStreams = value;
          this.#credit.wake();
        }
      }
      /** Fail every pending and future credit wait (connection teardown). */
      fail(err) {
        if (this.#failure)
          return;
        this.#failure = err;
        this.#credit.wake();
      }
      /** Wake blocked reservations so they re-check (stream credit or failure). */
      wake() {
        this.#credit.wake();
      }
      get failed() {
        return this.#failure;
      }
      /**
       * The peer's per-stream initial window. A getter (not copied into
       * StreamFlow) so streams staged before the peer's HELLO see the window the
       * moment it arrives.
       */
      get peerInitialMaxStreamData() {
        return this.#peerMaxStreamDataInitial;
      }
      newStream(sendMaxStreamData) {
        return new StreamFlow(this, this.local.initialMaxStreamData, sendMaxStreamData);
      }
      // ---- sender: byte credit (called via StreamFlow) ----
      /**
       * Reserve up to `n` DATA payload bytes at both levels, waiting until at
       * least one byte of credit is available (a writer larger than the whole
       * window must split at the window boundary, like a QUIC sender — an
       * all-or-nothing reservation would deadlock). Returns the granted amount
       * (1..n). Rejects if the stream's write half fails (STOP_SENDING, reset,
       * close), the connection fails, or `signal` aborts.
       */
      async reserveData(sf, n, signal) {
        for (; ; ) {
          if (this.#failure)
            throw this.#failure;
          const sfErr = sf.sendFailed;
          if (sfErr)
            throw sfErr;
          if (signal?.aborted)
            throw new Error("kps: aborted");
          const streamAvail = sf.peerMaxStreamData - sf.sendSent - sf.sendReserved;
          const connAvail = this.#peerMaxData - this.#connSent - this.#connReserved;
          let grant = streamAvail < connAvail ? streamAvail : connAvail;
          if (grant > n)
            grant = n;
          if (grant >= 1n) {
            sf.sendReserved += grant;
            this.#connReserved += grant;
            return grant;
          }
          await this.#credit.wait(signal);
        }
      }
      /** Bytes passed to the transport: reserved → sent, both levels. */
      commitData(sf, n) {
        sf.sendReserved -= n;
        sf.sendSent += n;
        this.#connReserved -= n;
        this.#connSent += n;
      }
      /** A reserved-but-unsent frame was discarded: release its reservation. */
      releaseData(sf, n) {
        sf.sendReserved -= n;
        this.#connReserved -= n;
        this.#credit.wake();
      }
      // ---- sender: stream slots ----
      /** Reserve a slot to open one stream, waiting at the limit. */
      async reserveStreamSlot(signal) {
        for (; ; ) {
          if (this.#failure)
            throw this.#failure;
          if (signal?.aborted)
            throw new Error("kps: aborted");
          if (this.#streamsOpened + this.#streamsReserved < this.#peerMaxStreams) {
            this.#streamsReserved += 1n;
            return;
          }
          await this.#credit.wait(signal);
        }
      }
      /** Channel creation succeeded: the cumulative count never decreases. */
      commitStreamSlot() {
        this.#streamsReserved -= 1n;
        this.#streamsOpened += 1n;
      }
      /** Channel creation failed synchronously: release the slot. */
      releaseStreamSlot() {
        this.#streamsReserved -= 1n;
        this.#credit.wake();
      }
      // ---- receiver: byte credit (called via StreamFlow) ----
      /** @throws ProtocolViolation when the peer exceeds the connection window. */
      connDataReceived(n) {
        if (this.#connReceived + n > this.#localMaxData) {
          throw new ProtocolViolation("peer exceeded MAX_DATA");
        }
        this.#connReceived += n;
      }
      connDataConsumed(n) {
        this.#connConsumed += n;
        const window2 = this.local.initialMaxData;
        if (this.#connConsumed - this.#connAdvertisedAt >= window2 / 2n) {
          this.#connAdvertisedAt = this.#connConsumed;
          this.#localMaxData = saturate(this.#connConsumed + window2);
          this.#sink.sendMaxData(this.#localMaxData);
        }
      }
      // ---- receiver: stream count ----
      /**
       * A peer-initiated stream was observed (it consumes a slot immediately, even
       * unaccepted or pre-HELLO).
       * @throws ProtocolViolation when the peer exceeds MAX_STREAMS.
       */
      peerStreamOpened() {
        if (this.#peerOpenedStreams >= this.#advertisedMaxStreams) {
          throw new ProtocolViolation("peer exceeded MAX_STREAMS");
        }
        this.#peerOpenedStreams += 1n;
      }
      /** A peer-initiated stream retired: grant a replacement slot. */
      peerStreamRetired() {
        this.#peerRetiredStreams += 1n;
        this.#advertisedMaxStreams = saturate(this.local.initialMaxStreams + this.#peerRetiredStreams);
        this.#sink.sendMaxStreams(this.#advertisedMaxStreams);
      }
    };
    StreamFlow = class {
      // sender side
      sendSent = 0n;
      sendReserved = 0n;
      #peerMaxExplicit = 0n;
      // largest MAX_STREAM_DATA received on this stream
      #sendFailure = null;
      // receiver side
      #localMaxStreamData;
      // enforcement limit
      #received = 0n;
      #consumed = 0n;
      #advertisedAt = 0n;
      #cancelled = false;
      // local cancelRead: no further stream credit
      #conn;
      #sendMaxStreamData;
      constructor(conn, localMaxStreamData, sendMaxStreamData) {
        this.#conn = conn;
        this.#localMaxStreamData = localMaxStreamData;
        this.#sendMaxStreamData = sendMaxStreamData;
      }
      /** Effective peer window: explicit updates never lower it below the HELLO initial. */
      get peerMaxStreamData() {
        const initial = this.#conn.peerInitialMaxStreamData;
        return this.#peerMaxExplicit > initial ? this.#peerMaxExplicit : initial;
      }
      // ---- sender ----
      /** Reserve up to `n` bytes; resolves with the granted amount (1..n). */
      async reserve(n, signal) {
        return Number(await this.#conn.reserveData(this, BigInt(n), signal));
      }
      commit(n) {
        this.#conn.commitData(this, BigInt(n));
      }
      release(n) {
        this.#conn.releaseData(this, BigInt(n));
      }
      /** Fail pending and future reservations (STOP_SENDING, reset, close). */
      failSend(err) {
        if (this.#sendFailure)
          return;
        this.#sendFailure = err;
        this.#conn.wake();
      }
      get sendFailed() {
        return this.#sendFailure;
      }
      /** MAX_STREAM_DATA from the peer. Decreases are ignored. */
      onPeerMaxStreamData(value) {
        if (value > this.#peerMaxExplicit) {
          this.#peerMaxExplicit = value;
          this.#conn.wake();
        }
      }
      // ---- receiver ----
      /**
       * `n` inbound DATA payload bytes arrived; enforce both windows atomically
       * (single-threaded: check both, then count both).
       * @throws ProtocolViolation when the peer exceeds either window.
       */
      onDataReceived(n) {
        const bn = BigInt(n);
        if (this.#received + bn > this.#localMaxStreamData) {
          throw new ProtocolViolation("peer exceeded MAX_STREAM_DATA");
        }
        this.#conn.connDataReceived(bn);
        this.#received += bn;
      }
      /**
       * `n` bytes were consumed — read-fulfilled to the application or explicitly
       * discarded. Advertises replacement credit past the half-window threshold
       * (stream credit is withheld after cancelRead; connection credit always
       * flows so a discarded stream cannot starve unrelated streams).
       */
      onConsumed(n) {
        const bn = BigInt(n);
        this.#consumed += bn;
        const window2 = this.#conn.local.initialMaxStreamData;
        if (!this.#cancelled && this.#consumed - this.#advertisedAt >= window2 / 2n) {
          this.#advertisedAt = this.#consumed;
          this.#localMaxStreamData = saturate(this.#consumed + window2);
          this.#sendMaxStreamData(this.#localMaxStreamData);
        }
        this.#conn.connDataConsumed(bn);
      }
      /** Local cancelRead: stop granting stream credit; discards still free MAX_DATA. */
      markCancelled() {
        this.#cancelled = true;
      }
    };
  }
});

// node_modules/@kpstreams/core/dist/stream-core.js
var LOCAL_SEND_BUFFER_LOW, Wakeable2, KpsStream;
var init_stream_core = __esm({
  "node_modules/@kpstreams/core/dist/stream-core.js"() {
    init_framing();
    init_errors2();
    LOCAL_SEND_BUFFER_LOW = 1 << 20;
    Wakeable2 = class {
      #resolvers = [];
      wake() {
        const rs = this.#resolvers;
        this.#resolvers = [];
        for (const r of rs)
          r();
      }
      wait() {
        return new Promise((res) => this.#resolvers.push(res));
      }
    };
    KpsStream = class {
      readable;
      writable;
      closed;
      /** Resolves when the channel opens; rejects if it dies first. */
      opened;
      #ch;
      #sf;
      #hooks;
      #inbuf = [];
      #peerFin = false;
      #peerReset = null;
      #peerStop = null;
      #localTerminal = null;
      #readCancelled = false;
      // The reason a locally-terminated read half surfaces to a pending/subsequent
      // read. Per SPEC §9.2, EOF is reserved for the peer's FIN; a local
      // cancelRead/close or a connection teardown must make the read *error*.
      #readError = null;
      #channelClosed = false;
      #retiredFired = false;
      #readWake = new Wakeable2();
      #drainWake = new Wakeable2();
      #closeResolve;
      #closeSettled = false;
      #openResolve;
      #openReject;
      #openSettled = false;
      constructor(ch, connFlow, hooks) {
        this.#ch = ch;
        this.#hooks = hooks;
        this.#sf = connFlow.newStream((v) => {
          if (this.#ch.isOpen())
            this.#ch.send(encodeMaxStreamData(v));
        });
        this.closed = new Promise((res) => {
          this.#closeResolve = res;
        });
        this.opened = new Promise((res, rej) => {
          this.#openResolve = res;
          this.#openReject = rej;
        });
        this.opened.catch(() => {
        });
        ch.setBufferedAmountLowThreshold(LOCAL_SEND_BUFFER_LOW);
        ch.onBufferedAmountLow(() => this.#drainWake.wake());
        ch.onOpen(() => this.#settleOpen(null));
        if (ch.isOpen())
          this.#settleOpen(null);
        ch.onMessage((d) => this.#onFrame(d));
        ch.onClose(() => this.#onChannelClose());
        ch.onError((msg) => {
          this.#settle({ ok: false, reason: { code: "network-error", message: msg } });
          this.#settleOpen(new Error(`kps: stream failed: ${msg}`));
          this.#readWake.wake();
          this.#drainWake.wake();
        });
        this.readable = new ReadableStream({
          pull: async (controller) => {
            for (; ; ) {
              if (this.#readCancelled) {
                controller.error(streamError(this.#readError ?? { code: "cancelled" }));
                return;
              }
              const chunk = this.#inbuf.shift();
              if (chunk) {
                controller.enqueue(chunk);
                this.#sf.onConsumed(chunk.length);
                this.#maybeRetire();
                return;
              }
              if (this.#peerReset) {
                controller.error(streamError(this.#peerReset));
                return;
              }
              if (this.#peerFin) {
                controller.close();
                return;
              }
              if (this.#channelClosed) {
                controller.error(streamError({ code: "network-error", message: "kps: stream closed" }));
                return;
              }
              await this.#readWake.wait();
            }
          },
          cancel: (reason) => {
            void this.cancelRead(reasonFrom(reason) ?? { code: "cancelled" });
          }
        }, { highWaterMark: 0 });
        this.writable = new WritableStream({
          write: (chunk) => this.#writeChunk(chunk),
          close: () => this.closeWrite(),
          abort: (reason) => this.resetWrite(reasonFrom(reason) ?? { code: "reset" })
        });
      }
      // ---- inbound ----
      #onFrame(data) {
        let f;
        try {
          f = parseFrame(data);
        } catch (e) {
          this.#hooks.fatal({ code: "protocol-error", message: e.message });
          return;
        }
        switch (f.type) {
          case "data": {
            if (this.#peerFin || this.#peerReset) {
              this.#hooks.fatal({ code: "protocol-error", message: "DATA after terminal frame" });
              return;
            }
            try {
              this.#sf.onDataReceived(f.payload.length);
            } catch (e) {
              this.#hooks.fatal({ code: "protocol-error", message: e.message });
              return;
            }
            if (this.#readCancelled) {
              this.#sf.onConsumed(f.payload.length);
              return;
            }
            this.#inbuf.push(f.payload.slice());
            this.#readWake.wake();
            return;
          }
          case "fin": {
            if (this.#peerFin || this.#peerReset) {
              this.#hooks.fatal({ code: "protocol-error", message: "second terminal frame" });
              return;
            }
            this.#peerFin = true;
            this.#readWake.wake();
            this.#maybeRetire();
            return;
          }
          case "reset": {
            if (this.#peerFin || this.#peerReset) {
              this.#hooks.fatal({ code: "protocol-error", message: "second terminal frame" });
              return;
            }
            this.#peerReset = { code: numToCode(f.code) ?? "reset" };
            this.#discardInbuf();
            this.#readWake.wake();
            this.#maybeRetire();
            return;
          }
          case "stop-sending": {
            if (this.#peerStop)
              return;
            this.#peerStop = { code: numToCode(f.code) ?? "cancelled" };
            this.#sf.failSend(streamError(this.#peerStop));
            if (!this.#localTerminal) {
              this.#localTerminal = "reset";
              if (this.#ch.isOpen())
                this.#ch.send(encodeCode(FRAME_RESET, f.code));
              this.#maybeRetire();
            }
            return;
          }
          case "max-stream-data":
            this.#sf.onPeerMaxStreamData(f.value);
            return;
        }
      }
      #onChannelClose() {
        this.#channelClosed = true;
        const wireComplete = this.#localTerminal !== null && (this.#peerFin || this.#peerReset !== null);
        if (!wireComplete && !this.#hooks.isTeardown()) {
          this.#hooks.fatal({ code: "protocol-error", message: "data channel closed mid-stream" });
        }
        this.#sf.failSend(streamError({ code: "network-error", message: "kps: stream closed" }));
        this.#settleOpen(new Error("kps: stream closed before opening"));
        this.#settle({ ok: !this.#peerReset, reason: this.#peerReset ?? void 0 });
        this.#readWake.wake();
        this.#drainWake.wake();
        this.#maybeRetire();
      }
      // ---- outbound ----
      async #writeChunk(chunk) {
        let off = 0;
        while (off < chunk.length) {
          this.#checkWritable();
          const want = Math.min(chunk.length - off, MAX_FRAME_PAYLOAD);
          const granted = await this.#sf.reserve(want);
          const slice2 = chunk.subarray(off, off + granted);
          try {
            await this.#drainLocal();
            this.#checkWritable();
            this.#ch.send(encodeData(slice2));
          } catch (e) {
            this.#sf.release(granted);
            throw e;
          }
          this.#sf.commit(granted);
          off += granted;
        }
      }
      #checkWritable() {
        if (this.#peerStop)
          throw streamError(this.#peerStop);
        if (this.#localTerminal)
          throw streamError({ code: "closed", message: "kps: write half closed" });
        if (this.#channelClosed || !this.#ch.isOpen())
          throw new Error("kps: stream is closed");
      }
      // Local send-queue bound only — flow control is the credit reservation above.
      async #drainLocal() {
        while (this.#ch.isOpen() && this.#ch.bufferedAmount() >= LOCAL_SEND_BUFFER_LOW) {
          await this.#drainWake.wait();
        }
      }
      // ---- public stream operations ----
      /** Gracefully finish the local write half; the peer observes EOF after all written bytes. */
      async closeWrite() {
        if (this.#localTerminal)
          return;
        this.#localTerminal = "fin";
        this.#sf.failSend(streamError({ code: "closed", message: "kps: write half closed" }));
        if (this.#ch.isOpen())
          this.#ch.send(encodeFin());
        this.#maybeRetire();
      }
      /** Stop wanting inbound bytes (not EOF); the peer is told to stop sending. */
      async cancelRead(reason) {
        if (this.#readCancelled)
          return;
        this.#readCancelled = true;
        this.#readError = reason ?? { code: "cancelled" };
        this.#sf.markCancelled();
        this.#discardInbuf();
        if (!this.#peerFin && !this.#peerReset && this.#ch.isOpen()) {
          this.#ch.send(encodeCode(FRAME_STOP_SENDING, codeToNum(reason?.code ?? "cancelled")));
        }
        this.#readWake.wake();
        this.#maybeRetire();
      }
      /** Abort the local write half; the peer observes a stream error rather than EOF. */
      async resetWrite(reason) {
        if (this.#localTerminal)
          return;
        this.#localTerminal = "reset";
        this.#sf.failSend(streamError(reason ?? { code: "reset" }));
        if (this.#ch.isOpen())
          this.#ch.send(encodeCode(FRAME_RESET, codeToNum(reason?.code ?? "reset")));
        this.#maybeRetire();
      }
      /**
       * Tear down both halves. The channel itself closes at retirement — once the
       * peer's terminal frame (a conforming peer answers STOP_SENDING with RESET)
       * has arrived — because closing it earlier is a §6.5 protocol violation.
       */
      async close(reason) {
        try {
          await this.closeWrite();
        } catch {
        }
        try {
          await this.cancelRead(reason ?? { code: "closed" });
        } catch {
        }
      }
      /** Connection teardown: discard state, fail waiters, no wire activity. */
      destroy(reason) {
        this.#discardInbuf();
        this.#readCancelled = true;
        this.#readError = reason ?? { code: "closed", message: "kps: connection closed" };
        this.#sf.failSend(streamError(reason ?? { code: "closed", message: "kps: connection closed" }));
        this.#settleOpen(new Error("kps: connection closed"));
        this.#settle(reason ? { ok: false, reason } : { ok: true });
        this.#readWake.wake();
        this.#drainWake.wake();
      }
      // ---- lifecycle ----
      #discardInbuf() {
        if (this.#inbuf.length === 0)
          return;
        let n = 0;
        for (const c of this.#inbuf)
          n += c.length;
        this.#inbuf = [];
        this.#sf.onConsumed(n);
      }
      #maybeRetire() {
        const wireComplete = this.#localTerminal !== null && (this.#peerFin || this.#peerReset !== null);
        if (!wireComplete)
          return;
        const drained = this.#inbuf.length === 0;
        if (!drained)
          return;
        if (!this.#channelClosed) {
          this.#ch.close();
          return;
        }
        if (!this.#retiredFired) {
          this.#retiredFired = true;
          this.#hooks.retired();
        }
      }
      #settle(info) {
        if (this.#closeSettled)
          return;
        this.#closeSettled = true;
        this.#closeResolve(info);
      }
      #settleOpen(err) {
        if (this.#openSettled)
          return;
        this.#openSettled = true;
        if (err)
          this.#openReject(err);
        else
          this.#openResolve();
      }
    };
  }
});

// node_modules/@kpstreams/core/dist/conn-core.js
function raceAbort(p, signal, message) {
  if (!signal)
    return p;
  if (signal.aborted)
    return Promise.reject(new Error(message));
  return new Promise((resolve, reject) => {
    const onAbort = () => reject(new Error(message));
    signal.addEventListener("abort", onAbort, { once: true });
    p.then((v) => {
      signal.removeEventListener("abort", onAbort);
      resolve(v);
    }, (e) => {
      signal.removeEventListener("abort", onAbort);
      reject(e);
    });
  });
}
var WEBRTC_MAX_DATAGRAM, CONTROL_LABEL, CONTROL_ID, DATAGRAM_LABEL, DATAGRAM_ID, DEFAULT_HELLO_TIMEOUT_MS, MAX_DATAGRAM_QUEUE, ConnCore;
var init_conn_core = __esm({
  "node_modules/@kpstreams/core/dist/conn-core.js"() {
    init_framing();
    init_control();
    init_flow();
    init_stream_core();
    init_errors2();
    WEBRTC_MAX_DATAGRAM = 1200;
    CONTROL_LABEL = "_kps_control";
    CONTROL_ID = 0;
    DATAGRAM_LABEL = "_kps_datagrams";
    DATAGRAM_ID = 1;
    DEFAULT_HELLO_TIMEOUT_MS = 15e3;
    MAX_DATAGRAM_QUEUE = 256;
    ConnCore = class {
      closed;
      /** Resolves at mutual HELLO; rejects if the connection dies first. */
      established;
      flow;
      #host;
      #state = "connecting";
      #tearingDown = false;
      #helloSent = false;
      #peerHello = null;
      #establishedDone = false;
      #establishResolve;
      #establishReject;
      #helloTimer;
      #seq = 0;
      #streams = /* @__PURE__ */ new Set();
      #staged = [];
      #incoming = [];
      #acceptWaiters = [];
      #dgQueue = [];
      #dgWaiters = [];
      #closeResolve;
      #closeFired = false;
      constructor(host) {
        this.#host = host;
        this.closed = new Promise((res) => {
          this.#closeResolve = res;
        });
        this.established = new Promise((res, rej) => {
          this.#establishResolve = res;
          this.#establishReject = rej;
        });
        this.established.catch(() => {
        });
        this.flow = new ConnFlow(resolveLimits(host.limits), {
          sendMaxData: (v) => this.#trySendControl(encodeMaxData(v)),
          sendMaxStreams: (v) => this.#trySendControl(encodeMaxStreams(v))
        });
        host.control.onOpen(() => this.#sendHello());
        host.control.onMessage((d) => this.#onControl(d));
        host.control.onClose(() => this.#reservedChannelLost("control"));
        host.control.onError(() => this.#reservedChannelLost("control"));
        if (host.control.isOpen())
          this.#sendHello();
        host.datagram.onMessage((d) => this.#onDatagram(d));
        host.datagram.onClose(() => this.#reservedChannelLost("datagram"));
        this.#helloTimer = setTimeout(() => this.fatal({ code: "timeout", message: "kps: HELLO timeout" }), host.helloTimeoutMs ?? DEFAULT_HELLO_TIMEOUT_MS);
        this.#helloTimer.unref?.();
      }
      get state() {
        return this.#state;
      }
      // ---- control channel ----
      #sendHello() {
        if (this.#helloSent || this.#closeFired)
          return;
        this.#helloSent = true;
        this.#trySendControl(encodeHello(this.flow.local));
        this.#checkEstablished();
      }
      #onControl(data) {
        let m;
        try {
          m = decodeControl(data);
        } catch (e) {
          this.fatal({ code: "protocol-error", message: e.message });
          return;
        }
        switch (m.t) {
          case "hello": {
            if (this.#peerHello) {
              this.fatal({ code: "protocol-error", message: "duplicate HELLO" });
              return;
            }
            if (m.version !== WIRE_VERSION) {
              this.#trySendControl(encodeConnClose("unsupported"));
              this.#teardown({
                ok: false,
                reason: { code: "unsupported", message: `kps: peer wire version ${m.version} (want ${WIRE_VERSION})` }
              });
              return;
            }
            this.#peerHello = m.limits;
            this.flow.onPeerHello(m.limits);
            this.#checkEstablished();
            return;
          }
          case "close": {
            const reason = m.code === 0 ? void 0 : { code: numToCode(m.code) ?? "internal-error" };
            this.#teardown({ ok: m.code === 0, reason });
            return;
          }
          case "max-data":
            if (!this.#peerHello) {
              this.fatal({ code: "protocol-error", message: "control message before HELLO" });
              return;
            }
            this.flow.onPeerMaxData(m.value);
            return;
          case "max-streams":
            if (!this.#peerHello) {
              this.fatal({ code: "protocol-error", message: "control message before HELLO" });
              return;
            }
            this.flow.onPeerMaxStreams(m.value);
            return;
        }
      }
      #checkEstablished() {
        if (this.#establishedDone || this.#closeFired)
          return;
        if (!this.#helloSent || !this.#peerHello)
          return;
        this.#establishedDone = true;
        this.#state = "open";
        clearTimeout(this.#helloTimer);
        this.#establishResolve();
        const staged = this.#staged;
        this.#staged = [];
        for (const s of staged)
          this.#enqueueIncoming(s);
      }
      #trySendControl(msg) {
        try {
          if (this.#host.control.isOpen())
            this.#host.control.send(msg);
        } catch {
        }
      }
      #reservedChannelLost(which) {
        if (this.#tearingDown || this.#closeFired)
          return;
        this.fatal({ code: "protocol-error", message: `kps: reserved ${which} channel lost` });
      }
      // ---- streams ----
      /** The wrapper calls this for every incoming (DCEP) application channel. */
      handleIncomingChannel(ch) {
        try {
          this.flow.peerStreamOpened();
        } catch (e) {
          this.fatal({ code: "protocol-error", message: e.message });
          return;
        }
        const stream = this.#makeStream(ch, true);
        if (this.#establishedDone)
          this.#enqueueIncoming(stream);
        else
          this.#staged.push(stream);
      }
      #makeStream(ch, peerInitiated) {
        const stream = new KpsStream(ch, this.flow, {
          fatal: (r) => this.fatal(r),
          retired: () => {
            this.#streams.delete(stream);
            if (peerInitiated)
              this.flow.peerStreamRetired();
          },
          isTeardown: () => this.#tearingDown
        });
        this.#streams.add(stream);
        return stream;
      }
      #enqueueIncoming(stream) {
        const w = this.#acceptWaiters.shift();
        if (w)
          w.resolve(stream);
        else
          this.#incoming.push(stream);
      }
      async openStream(opts = {}) {
        if (opts.signal?.aborted)
          throw new Error("kps: openStream aborted");
        if (this.#state !== "open")
          throw new Error(`kps: connection is ${this.#state}`);
        await this.flow.reserveStreamSlot(opts.signal);
        let ch;
        try {
          ch = this.#host.openChannel(`kps-${++this.#seq}`);
        } catch (e) {
          this.flow.releaseStreamSlot();
          throw e;
        }
        this.flow.commitStreamSlot();
        const stream = this.#makeStream(ch, false);
        try {
          await raceAbort(stream.opened, opts.signal, "kps: openStream aborted");
        } catch (e) {
          stream.opened.then(() => {
            void stream.resetWrite({ code: "cancelled" });
            void stream.cancelRead({ code: "cancelled" });
          }).catch(() => {
          });
          throw e;
        }
        return stream;
      }
      acceptStream(opts = {}) {
        const ready = this.#incoming.shift();
        if (ready)
          return Promise.resolve(ready);
        if (opts.signal?.aborted)
          return Promise.reject(new Error("kps: acceptStream aborted"));
        if (this.#state === "closed")
          return Promise.reject(new Error("kps: connection is closed"));
        const signal = opts.signal;
        return new Promise((resolve, reject) => {
          const waiter = {
            resolve: (s) => {
              signal?.removeEventListener("abort", onAbort);
              resolve(s);
            },
            reject: (e) => {
              signal?.removeEventListener("abort", onAbort);
              reject(e);
            }
          };
          const onAbort = () => {
            const i = this.#acceptWaiters.indexOf(waiter);
            if (i >= 0)
              this.#acceptWaiters.splice(i, 1);
            reject(new Error("kps: acceptStream aborted"));
          };
          this.#acceptWaiters.push(waiter);
          signal?.addEventListener("abort", onAbort, { once: true });
        });
      }
      // ---- datagrams (SPEC §7) ----
      #onDatagram(data) {
        const w = this.#dgWaiters.shift();
        if (w) {
          w.resolve(data);
          return;
        }
        this.#dgQueue.push(data);
        if (this.#dgQueue.length > MAX_DATAGRAM_QUEUE)
          this.#dgQueue.shift();
      }
      async sendDatagram(data, opts) {
        if (opts?.signal?.aborted)
          throw new Error("kps: sendDatagram aborted");
        if (data.length > WEBRTC_MAX_DATAGRAM) {
          const e = new Error(`kps: datagram exceeds limit (max ${WEBRTC_MAX_DATAGRAM} bytes)`);
          Object.assign(e, { code: "too-large", maxDatagramPayloadSize: WEBRTC_MAX_DATAGRAM });
          throw e;
        }
        if (!this.#host.datagram.isOpen())
          throw new Error("kps: datagram channel not open");
        this.#host.datagram.send(data);
      }
      receiveDatagram(opts) {
        const next = this.#dgQueue.shift();
        if (next)
          return Promise.resolve(next);
        if (this.#state === "closed")
          return Promise.reject(new Error("kps: connection closed"));
        if (opts?.signal?.aborted)
          return Promise.reject(new Error("kps: receiveDatagram aborted"));
        const signal = opts?.signal;
        return new Promise((resolve, reject) => {
          const waiter = {
            resolve: (v) => {
              signal?.removeEventListener("abort", onAbort);
              resolve(v);
            },
            reject: (e) => {
              signal?.removeEventListener("abort", onAbort);
              reject(e);
            }
          };
          const onAbort = () => {
            const i = this.#dgWaiters.indexOf(waiter);
            if (i >= 0)
              this.#dgWaiters.splice(i, 1);
            reject(new Error("kps: receiveDatagram aborted"));
          };
          this.#dgWaiters.push(waiter);
          signal?.addEventListener("abort", onAbort, { once: true });
        });
      }
      // ---- close paths ----
      /** Graceful local close: best-effort CONNECTION_CLOSE, then teardown. */
      close(reason) {
        if (this.#closeFired)
          return;
        this.#tearingDown = true;
        this.#trySendControl(encodeConnClose(reason?.code));
        this.#teardown({ ok: true, reason });
      }
      /** A peer wire violation or local fatal condition: convey a code, tear down. */
      fatal(reason) {
        if (this.#closeFired)
          return;
        this.#tearingDown = true;
        this.#trySendControl(encodeConnClose(reason.code ?? "protocol-error"));
        this.#teardown({ ok: false, reason });
      }
      /** Transport-layer state changes, forwarded by the wrapper. */
      onTransportFailed(message = "peer connection failed") {
        this.#teardown({ ok: false, reason: { code: "network-error", message } });
      }
      onTransportClosed() {
        this.#teardown({ ok: this.#state !== "connecting" });
      }
      #teardown(info) {
        if (this.#closeFired)
          return;
        this.#closeFired = true;
        this.#tearingDown = true;
        this.#state = "closed";
        clearTimeout(this.#helloTimer);
        const err = info.reason ? streamError(info.reason) : new Error("kps: connection closed");
        if (!this.#establishedDone)
          this.#establishReject(err);
        this.flow.fail(err);
        for (const s of [...this.#streams, ...this.#staged])
          s.destroy(info.reason);
        this.#streams.clear();
        this.#staged = [];
        for (const w of this.#acceptWaiters)
          w.reject(new Error("kps: connection closed"));
        this.#acceptWaiters = [];
        for (const w of this.#dgWaiters)
          w.reject(new Error("kps: connection closed"));
        this.#dgWaiters = [];
        try {
          this.#host.closeTransport();
        } catch {
        }
        this.#closeResolve(info);
      }
    };
  }
});

// node_modules/@kpstreams/core/dist/sdp.js
function generateUfrag() {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
async function deriveICEPwd(certhashDigest, ufrag) {
  const key = await crypto.subtle.importKey("raw", toArrayBuffer(certhashDigest), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const msg = new TextEncoder().encode("kps-ice-pwd-v1:" + ufrag);
  const sig = new Uint8Array(await crypto.subtle.sign("HMAC", key, toArrayBuffer(msg)));
  let bin = "";
  for (const b of sig)
    bin += String.fromCharCode(b);
  return btoa(bin).replace(/=+$/, "");
}
function toArrayBuffer(u8) {
  return u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength);
}
function rewriteOfferUfrag(sdp, ufrag, pwd) {
  const lines = sdp.split(/\r\n|\n/).map((line) => {
    if (line.startsWith("a=ice-ufrag:"))
      return `a=ice-ufrag:${ufrag}`;
    if (line.startsWith("a=ice-pwd:"))
      return `a=ice-pwd:${pwd}`;
    return line;
  });
  return lines.join("\r\n");
}
function synthesizeAnswer(addr, ufrag, pwd) {
  const fingerprint = digestToSdpFingerprint(decodeCerthash(addr.certhash));
  const ip6 = addr.ip.includes(":");
  const fam = ip6 ? "IP6" : "IP4";
  const lines = [
    "v=0",
    `o=- 0 0 IN ${fam} ${ip6 ? "::" : "0.0.0.0"}`,
    "s=-",
    "t=0 0",
    "a=ice-lite",
    `m=application ${addr.port} UDP/DTLS/SCTP webrtc-datachannel`,
    `c=IN ${fam} ${addr.ip}`,
    "a=mid:0",
    `a=ice-ufrag:${ufrag}`,
    `a=ice-pwd:${pwd}`,
    `a=fingerprint:sha-256 ${fingerprint}`,
    "a=setup:passive",
    "a=sctp-port:5000",
    "a=max-message-size:1048576",
    `a=candidate:1 1 UDP 1 ${addr.ip} ${addr.port} typ host`
  ];
  return lines.join("\r\n") + "\r\n";
}
var init_sdp = __esm({
  "node_modules/@kpstreams/core/dist/sdp.js"() {
    init_certhash();
  }
});

// node_modules/@kpstreams/core/dist/webrtc.js
var init_webrtc = __esm({
  "node_modules/@kpstreams/core/dist/webrtc.js"() {
    init_conn_core();
    init_sdp();
  }
});

// node_modules/@kpstreams/webrtc-client/dist/connection.js
function toArrayBuffer2(u8) {
  return u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength);
}
function dialAbortError(signal) {
  const reason = signal.reason;
  return new Error(reason?.name === "TimeoutError" ? "kps: dial timed out" : "kps: dial aborted");
}
function dial(addr, opts) {
  return Connection.dial(addr, opts);
}
var DEFAULT_TIMEOUT, RTCChannelAdapter, Connection;
var init_connection = __esm({
  "node_modules/@kpstreams/webrtc-client/dist/connection.js"() {
    init_dist2();
    init_webrtc();
    DEFAULT_TIMEOUT = 15e3;
    RTCChannelAdapter = class {
      #dc;
      #message = null;
      #open = null;
      #close = null;
      #error = null;
      #bal = null;
      constructor(dc) {
        this.#dc = dc;
        dc.binaryType = "arraybuffer";
        dc.addEventListener("message", (e) => {
          const raw2 = e.data;
          const data = typeof raw2 === "string" ? new TextEncoder().encode(raw2) : new Uint8Array(raw2);
          this.#message?.(data);
        });
        dc.addEventListener("open", () => this.#open?.());
        dc.addEventListener("close", () => this.#close?.());
        dc.addEventListener("error", (e) => {
          this.#error?.(e.error?.message ?? "data channel error");
        });
        dc.addEventListener("bufferedamountlow", () => this.#bal?.());
      }
      isOpen() {
        return this.#dc.readyState === "open";
      }
      send(data) {
        this.#dc.send(toArrayBuffer2(data));
      }
      bufferedAmount() {
        return this.#dc.bufferedAmount;
      }
      setBufferedAmountLowThreshold(bytes) {
        this.#dc.bufferedAmountLowThreshold = bytes;
      }
      onBufferedAmountLow(cb) {
        this.#bal = cb;
      }
      onMessage(cb) {
        this.#message = cb;
      }
      onOpen(cb) {
        this.#open = cb;
      }
      onClose(cb) {
        this.#close = cb;
      }
      onError(cb) {
        this.#error = cb;
      }
      close() {
        try {
          this.#dc.close();
        } catch {
        }
      }
    };
    Connection = class _Connection {
      // The dialed endpoint (see the core Connection.remoteAddress doc).
      remoteAddress;
      #pc;
      #core;
      // `control` is the reserved reliable channel (ID 0) dial() created before the
      // offer (to force the SCTP m-line).
      constructor(pc, control, remote) {
        this.#pc = pc;
        this.remoteAddress = remote;
        const dg = pc.createDataChannel(DATAGRAM_LABEL, {
          negotiated: true,
          id: DATAGRAM_ID,
          ordered: false,
          maxRetransmits: 0
        });
        this.#core = new ConnCore({
          control: new RTCChannelAdapter(control),
          datagram: new RTCChannelAdapter(dg),
          openChannel: (label) => new RTCChannelAdapter(pc.createDataChannel(label)),
          closeTransport: () => {
            try {
              pc.close();
            } catch {
            }
          }
        });
        pc.addEventListener("connectionstatechange", () => {
          const s = pc.connectionState;
          if (s === "failed")
            this.#core.onTransportFailed();
          else if (s === "closed")
            this.#core.onTransportClosed();
        });
        pc.addEventListener("datachannel", (e) => {
          const channel = e.channel;
          if (channel.label === CONTROL_LABEL || channel.label === DATAGRAM_LABEL)
            return;
          this.#core.handleIncomingChannel(new RTCChannelAdapter(channel));
        });
      }
      get closed() {
        return this.#core.closed;
      }
      static async dial(addrStr, opts = {}) {
        const signal = opts.signal ?? AbortSignal.timeout(DEFAULT_TIMEOUT);
        if (signal.aborted)
          throw dialAbortError(signal);
        const addr = parseAddress(addrStr);
        const digest = decodeCerthash(addr.certhash);
        const pc = new RTCPeerConnection({});
        const control = pc.createDataChannel(CONTROL_LABEL, { negotiated: true, id: CONTROL_ID });
        const offer = await pc.createOffer();
        const ufrag = generateUfrag();
        const pwd = await deriveICEPwd(digest, ufrag);
        await pc.setLocalDescription({ type: offer.type, sdp: rewriteOfferUfrag(offer.sdp ?? "", ufrag, pwd) });
        await pc.setRemoteDescription({ type: "answer", sdp: synthesizeAnswer(addr, ufrag, pwd) });
        const conn = new _Connection(pc, control, { ip: addr.ip, port: addr.port });
        await conn.#waitEstablished(signal);
        return conn;
      }
      #waitEstablished(signal) {
        return new Promise((resolve, reject) => {
          const onAbort = () => {
            try {
              this.#pc.close();
            } catch {
            }
            reject(dialAbortError(signal));
          };
          signal.addEventListener("abort", onAbort, { once: true });
          this.#core.established.then(() => {
            signal.removeEventListener("abort", onAbort);
            resolve();
          }, (e) => {
            signal.removeEventListener("abort", onAbort);
            reject(e);
          });
        });
      }
      openStream(opts = {}) {
        return this.#core.openStream(opts);
      }
      acceptStream(opts = {}) {
        return this.#core.acceptStream(opts);
      }
      async close(reason) {
        this.#core.close(reason);
      }
      // Datagrams (SPEC §7) — unreliable, unordered, best-effort.
      sendDatagram(data, opts) {
        return this.#core.sendDatagram(data, opts);
      }
      receiveDatagram(opts) {
        return this.#core.receiveDatagram(opts);
      }
    };
  }
});

// node_modules/@kpstreams/webrtc-client/dist/open-stream.js
async function openStream(addr, opts) {
  const conn = await dial(addr, opts);
  try {
    const stream = await conn.openStream({ signal: opts?.signal });
    void stream.closed.finally(() => {
      void conn.close();
    });
    return stream;
  } catch (err) {
    await conn.close();
    throw err;
  }
}
var init_open_stream = __esm({
  "node_modules/@kpstreams/webrtc-client/dist/open-stream.js"() {
    init_connection();
  }
});

// node_modules/@kpstreams/webrtc-client/dist/index.js
var dist_exports = {};
__export(dist_exports, {
  dial: () => dial,
  formatAddress: () => formatAddress,
  openStream: () => openStream,
  parseAddress: () => parseAddress
});
var init_dist3 = __esm({
  "node_modules/@kpstreams/webrtc-client/dist/index.js"() {
    init_connection();
    init_open_stream();
    init_dist2();
  }
});

// node_modules/@anon-rpc/browser-harness/dist/host.js
var host_exports = {};
__export(host_exports, {
  AnonRpcWorker: () => AnonRpcWorker,
  RpcError: () => RpcError
});
function abortError() {
  return new DOMException("The operation was aborted.", "AbortError");
}
function registerKpsBridge(rpc) {
  const conns = /* @__PURE__ */ new Map();
  const streams = /* @__PURE__ */ new Map();
  let nextId = 1;
  const addStream = (s) => {
    const streamId = nextId++;
    streams.set(streamId, s);
    void s.closed.then((info) => {
      streams.delete(streamId);
      rpc.emit("stream.closed", { streamId, info });
    });
    return {
      value: { streamId, readable: s.readable, writable: s.writable },
      transfer: [s.readable, s.writable]
    };
  };
  const conn = (id) => {
    const c = conns.get(id);
    if (!c) throw new Error(`unknown connId ${id}`);
    return c;
  };
  const stream = (id) => {
    const s = streams.get(id);
    if (!s) throw new Error(`unknown streamId ${id}`);
    return s;
  };
  rpc.on("kps.dial", async ({ addr }, { signal }) => {
    const c = await dial(addr, { signal });
    const connId = nextId++;
    conns.set(connId, c);
    void c.closed.then((info) => {
      conns.delete(connId);
      rpc.emit("conn.closed", { connId, info });
    });
    return { value: { connId, remoteAddress: c.remoteAddress } };
  });
  rpc.on("kps.openStream", async ({ addr }, { signal }) => {
    const s = await openStream(addr, { signal });
    return addStream(s);
  });
  rpc.on("conn.openStream", async ({ connId }, { signal }) => {
    return addStream(await conn(connId).openStream({ signal }));
  });
  rpc.on("conn.acceptStream", async ({ connId }, { signal }) => {
    return addStream(await conn(connId).acceptStream({ signal }));
  });
  rpc.on("conn.close", async ({ connId, reason }) => {
    await conn(connId).close(reason);
    conns.delete(connId);
    return { value: void 0 };
  });
  rpc.on("conn.sendDatagram", async ({ connId, data }, { signal }) => {
    await conn(connId).sendDatagram(data, { signal });
    return { value: void 0 };
  });
  rpc.on("conn.receiveDatagram", async ({ connId }, { signal }) => {
    return { value: await conn(connId).receiveDatagram({ signal }) };
  });
  rpc.on("stream.closeWrite", async ({ streamId }) => {
    await stream(streamId).closeWrite();
    return { value: void 0 };
  });
  rpc.on("stream.cancelRead", async ({ streamId, reason }) => {
    await stream(streamId).cancelRead(reason);
    return { value: void 0 };
  });
  rpc.on("stream.resetWrite", async ({ streamId, reason }) => {
    await stream(streamId).resetWrite(reason);
    return { value: void 0 };
  });
  rpc.on("stream.close", async ({ streamId, reason }) => {
    await stream(streamId).close(reason);
    streams.delete(streamId);
    return { value: void 0 };
  });
  return () => {
    for (const s of streams.values()) void s.close({ code: "closed" }).catch(() => {
    });
    for (const c of conns.values()) void c.close({ code: "closed" });
    streams.clear();
    conns.clear();
  };
}
async function normalizeRequest(input, init) {
  const forwarded = copyForwardedEntries(init);
  if (input instanceof Request) {
    const needsDuplex = init?.body instanceof ReadableStream;
    const req2 = init ? new Request(input, { ...needsDuplex ? { duplex: "half" } : {}, ...init }) : input;
    const headers2 = [];
    req2.headers.forEach((v, k) => headers2.push([k, v]));
    const requestInit2 = {
      method: req2.method,
      headers: headers2,
      redirect: req2.redirect,
      hasSignal: true
      // a Request always has a signal
    };
    if (forwarded) requestInit2.forwarded = forwarded;
    const transfer2 = [];
    if (req2.body) {
      requestInit2.body = req2.body;
      transfer2.push(req2.body);
    }
    return { url: req2.url, requestInit: requestInit2, transfer: transfer2, signal: req2.signal };
  }
  const url = typeof input === "string" ? input : input.href;
  if (!init) return { url };
  const src = init;
  const transfer = [];
  let body;
  let autoContentType;
  if (src.body != null) {
    if (src.body instanceof ReadableStream) {
      body = src.body;
      transfer.push(src.body);
    } else if (typeof src.body === "string") {
      body = new TextEncoder().encode(src.body);
      transfer.push(body.buffer);
    } else if (src.body instanceof Uint8Array || src.body instanceof ArrayBuffer) {
      body = src.body instanceof Uint8Array ? src.body : new Uint8Array(src.body);
    } else {
      const tmp = new Response(src.body);
      body = new Uint8Array(await tmp.arrayBuffer());
      autoContentType = tmp.headers.get("content-type") ?? void 0;
      transfer.push(body.buffer);
    }
  }
  const requestInit = {};
  if (src.method) requestInit.method = src.method;
  const headers = headersToList(src.headers) ?? [];
  if (autoContentType && !headers.some(([k]) => k.toLowerCase() === "content-type")) {
    headers.push(["content-type", autoContentType]);
  }
  if (headers.length) requestInit.headers = headers;
  if (body !== void 0) requestInit.body = body;
  if (src.redirect) requestInit.redirect = src.redirect;
  if (src.signal) requestInit.hasSignal = true;
  if (forwarded) requestInit.forwarded = forwarded;
  return { url, requestInit, transfer, signal: src.signal ?? void 0 };
}
function headersToList(h) {
  if (!h) return void 0;
  const out2 = [];
  if (h instanceof Headers) h.forEach((v, k) => out2.push([k, v]));
  else if (Array.isArray(h)) for (const [k, v] of h) out2.push([k, v]);
  else for (const [k, v] of Object.entries(h)) out2.push([k, String(v)]);
  return out2;
}
function openStorageBackend() {
  if (typeof indexedDB === "undefined") {
    return Promise.resolve(new MemoryBackend());
  }
  return IdbBackend.open();
}
function req(r) {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}
function parseKpsResolver(entry) {
  if (!entry.startsWith("kps:")) return void 0;
  const rest = entry.slice("kps:".length);
  const slash = rest.indexOf("/");
  if (slash <= 0) throw new Error("malformed kps resolver: missing /path");
  const addr = rest.slice(0, slash);
  const path = rest.slice(slash);
  if (addr.split(":").length < 3) throw new Error("malformed kps resolver: bad address");
  return { addr, path };
}
function buildKpsHttpRequest(path, certhash, acceptEncodings = []) {
  const accept = acceptEncodings.length ? `Accept-Encoding: ${acceptEncodings.join(", ")}${CRLF}` : "";
  return enc.encode(`GET ${path} HTTP/1.1${CRLF}Host: ${certhash}${CRLF}${accept}${CRLF}`);
}
function decompressionCtor() {
  return globalThis.DecompressionStream;
}
function decompressionFormatFor(coding) {
  const DS = decompressionCtor();
  if (!DS) return void 0;
  for (const format of CODING_FORMATS.find(([c]) => c === coding)?.[1] ?? []) {
    try {
      new DS(format);
      return format;
    } catch {
    }
  }
  return void 0;
}
function ambientCodings() {
  return CODING_FORMATS.map(([c]) => c).filter((c) => decompressionFormatFor(c) !== void 0);
}
async function decodeBody(coding, body, maxBytes) {
  const DS = decompressionCtor();
  const format = decompressionFormatFor(coding);
  if (!DS || format === void 0) throw new Error(`undecodable Content-Encoding: ${coding}`);
  const decoded = new Blob([body]).stream().pipeThrough(new DS(format));
  const reader = decoded.getReader();
  const chunks = [];
  let total = 0;
  for (; ; ) {
    const { value, done } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > maxBytes) {
      await reader.cancel().catch(() => {
      });
      throw new Error(`decoded body exceeds the ${maxBytes}-byte bundle cap`);
    }
    chunks.push(value);
  }
  const out2 = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) {
    out2.set(c, off);
    off += c.byteLength;
  }
  return out2;
}
function parseKpsHttpResponse(bytes) {
  const sep = findHeaderEnd(bytes);
  if (sep < 0) throw new Error("no header terminator in response");
  const head = new TextDecoder("utf-8", { fatal: false }).decode(bytes.subarray(0, sep));
  const lines = head.split(CRLF);
  const m = /^HTTP\/1\.1 (\d{3})(?: |$)/.exec(lines[0]);
  if (!m) throw new Error(`malformed status line: ${JSON.stringify(lines[0].slice(0, 80))}`);
  const status = Number(m[1]);
  const headers = [];
  for (const line of lines.slice(1)) {
    if (line === "") continue;
    const colon = line.indexOf(":");
    if (colon <= 0) throw new Error(`malformed header line: ${JSON.stringify(line.slice(0, 80))}`);
    const name = line.slice(0, colon).trim().toLowerCase();
    const value = line.slice(colon + 1).trim();
    if (name === "transfer-encoding") throw new Error("forbidden Transfer-Encoding in response");
    headers.push([name, value]);
  }
  return { status, headers, body: bytes.subarray(sep + 4) };
}
function findHeaderEnd(b) {
  for (let i = 0; i + 3 < b.length; i++) {
    if (b[i] === 13 && b[i + 1] === 10 && b[i + 2] === 13 && b[i + 3] === 10) return i;
  }
  return -1;
}
async function fetchBundleOverKps(addr, path, maxBytes) {
  const { openStream: openStream2 } = await Promise.resolve().then(() => (init_dist3(), dist_exports));
  const stream = await openStream2(addr);
  try {
    const certhash = addr.slice(addr.lastIndexOf(":") + 1);
    const advertised = ambientCodings();
    const writer = stream.writable.getWriter();
    await writer.write(buildKpsHttpRequest(path, certhash, advertised));
    await writer.close();
    const reader = stream.readable.getReader();
    const chunks = [];
    let total = 0;
    const headroom = 64 * 1024;
    for (; ; ) {
      const { value, done } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > maxBytes + headroom) {
        await reader.cancel().catch(() => {
        });
        throw new Error(`response exceeds the ${maxBytes}-byte bundle cap`);
      }
      chunks.push(value);
    }
    const all = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) {
      all.set(c, off);
      off += c.byteLength;
    }
    const resp = parseKpsHttpResponse(all);
    if (resp.status !== 200) throw new Error(`HTTP ${resp.status}`);
    const coding = resp.headers.find(([n]) => n === "content-encoding")?.[1].toLowerCase();
    if (coding && coding !== "identity") {
      if (!advertised.includes(coding)) {
        throw new Error(`unadvertised Content-Encoding: ${coding}`);
      }
      return decodeBody(coding, resp.body, maxBytes);
    }
    if (resp.body.byteLength > maxBytes) {
      throw new Error(`body exceeds the ${maxBytes}-byte bundle cap`);
    }
    return resp.body;
  } finally {
    await stream.close().catch(() => {
    });
  }
}
function sameOriginIframeUrl(raw2) {
  let url;
  try {
    url = new URL(raw2, location.href);
  } catch {
    throw new Error(`iframeUrl is not a valid URL: ${raw2}`);
  }
  if (url.origin !== location.origin) {
    throw new Error(
      `iframeUrl must be same-origin with the host (${location.origin}), got ${url.origin} \u2014 a cross-origin bootstrap would control the isolation boundary (\xA76)`
    );
  }
  return url.href;
}
function renderLogArg(a) {
  if (a instanceof Uint8Array) return `<${a.byteLength} bytes>`;
  return a;
}
function toResponse(r) {
  const headers = new Headers();
  for (const [k, v] of r.headers) headers.append(k, v);
  const status = r.status;
  const body = status === 204 || status === 205 || status === 304 ? null : r.body;
  const resp = new Response(body, { status, headers });
  if (r.url) Object.defineProperty(resp, "url", { value: r.url });
  return resp;
}
var PortRpc, DB_NAME, STORE_NAME, IdbBackend, MemoryBackend, enc, CRLF, CODING_FORMATS, kpsResolverKind, LOG_RETENTION, AnonRpcWorker;
var init_host = __esm({
  "node_modules/@anon-rpc/browser-harness/dist/host.js"() {
    init_dist();
    init_dist3();
    init_dist();
    init_dist();
    PortRpc = class {
      #port;
      #nextId = 1;
      #pending = /* @__PURE__ */ new Map();
      #handlers = /* @__PURE__ */ new Map();
      #events = /* @__PURE__ */ new Map();
      // AbortControllers for in-flight inbound requests, keyed by caller-side id.
      #inbound = /* @__PURE__ */ new Map();
      constructor(port) {
        this.#port = port;
        port.onmessage = (ev) => this.#onMessage(ev.data);
        port.start?.();
      }
      /** Register a handler for an inbound method. */
      on(method, handler) {
        this.#handlers.set(method, handler);
      }
      /** Register a handler for an inbound fire-and-forget event. */
      onEvent(topic, handler) {
        this.#events.set(topic, handler);
      }
      /** Fire-and-forget event to the peer. */
      emit(topic, data, transfer) {
        this.#post({ t: "evt", topic, data }, transfer);
      }
      /** Call a remote method and await its result. */
      call(method, args, opts) {
        const id = this.#nextId++;
        return new Promise((resolve, reject) => {
          let cleanup;
          if (opts?.signal) {
            const signal = opts.signal;
            if (signal.aborted) {
              reject(abortError());
              return;
            }
            const onAbort = () => {
              if (!this.#pending.delete(id)) return;
              this.#post({ t: "abort", id });
              reject(abortError());
            };
            signal.addEventListener("abort", onAbort);
            cleanup = () => signal.removeEventListener("abort", onAbort);
          }
          this.#pending.set(id, {
            resolve,
            reject,
            cleanup
          });
          this.#post({ t: "req", id, method, args }, opts?.transfer);
        });
      }
      #post(msg, transfer) {
        this.#port.postMessage(msg, transfer ?? []);
      }
      async #onMessage(msg) {
        switch (msg.t) {
          case "evt": {
            this.#events.get(msg.topic)?.(msg.data);
            return;
          }
          case "abort": {
            this.#inbound.get(msg.id)?.abort(abortError());
            return;
          }
          case "res": {
            const p = this.#pending.get(msg.id);
            if (!p) return;
            this.#pending.delete(msg.id);
            p.cleanup?.();
            if (msg.ok) p.resolve(msg.value);
            else p.reject(new RpcError(msg.error));
            return;
          }
          case "req": {
            const handler = this.#handlers.get(msg.method);
            if (!handler) {
              this.#post({
                t: "res",
                id: msg.id,
                ok: false,
                error: { name: "Error", message: `no handler: ${msg.method}` }
              });
              return;
            }
            const ac = new AbortController();
            this.#inbound.set(msg.id, ac);
            try {
              const result = await handler(msg.args, { signal: ac.signal });
              this.#post(
                { t: "res", id: msg.id, ok: true, value: result.value },
                result.transfer
              );
            } catch (err) {
              this.#post({
                t: "res",
                id: msg.id,
                ok: false,
                error: serializeError(err)
              });
            } finally {
              this.#inbound.delete(msg.id);
            }
            return;
          }
        }
      }
    };
    DB_NAME = "anon-rpc-harness";
    STORE_NAME = "kv";
    IdbBackend = class _IdbBackend {
      #db;
      constructor(db) {
        this.#db = db;
      }
      static async open() {
        const open = indexedDB.open(DB_NAME, 1);
        open.onupgradeneeded = () => {
          open.result.createObjectStore(STORE_NAME);
        };
        return new _IdbBackend(await req(open));
      }
      #store(mode) {
        return this.#db.transaction(STORE_NAME, mode).objectStore(STORE_NAME);
      }
      async get(address, key) {
        return await req(this.#store("readonly").get([address, key]));
      }
      async set(address, key, value) {
        await req(this.#store("readwrite").put(value, [address, key]));
      }
      async delete(address, key) {
        await req(this.#store("readwrite").delete([address, key]));
      }
      async has(address, key) {
        return await req(this.#store("readonly").getKey([address, key])) !== void 0;
      }
      async listKeys(address, prefix = "") {
        const keys = [];
        await this.#iterate("readonly", address, prefix, (cursor) => {
          keys.push(cursor.key[1]);
        });
        return keys;
      }
      async clear(address, prefix = "") {
        await this.#iterate("readwrite", address, prefix, (cursor) => {
          cursor.delete();
        });
      }
      // Walk the [address, prefix…] key range. The walk starts at the range's
      // lower bound and stops manually at the first key past the namespace or
      // prefix — exact for any key string, unlike the `prefix + "￿"` upper-
      // bound trick.
      #iterate(mode, address, prefix, visit) {
        return new Promise((resolve, reject) => {
          const store = this.#store(mode);
          const range = IDBKeyRange.lowerBound([address, prefix]);
          const r = mode === "readwrite" ? store.openCursor(range) : store.openKeyCursor(range);
          r.onsuccess = () => {
            const cursor = r.result;
            if (!cursor) return resolve();
            const [addr, key] = cursor.key;
            if (addr !== address || !key.startsWith(prefix)) return resolve();
            visit(cursor);
            cursor.continue();
          };
          r.onerror = () => reject(r.error);
        });
      }
    };
    MemoryBackend = class {
      #spaces = /* @__PURE__ */ new Map();
      #space(address) {
        let m = this.#spaces.get(address);
        if (!m) this.#spaces.set(address, m = /* @__PURE__ */ new Map());
        return m;
      }
      async get(address, key) {
        return this.#space(address).get(key);
      }
      async set(address, key, value) {
        this.#space(address).set(key, value);
      }
      async delete(address, key) {
        this.#space(address).delete(key);
      }
      async has(address, key) {
        return this.#space(address).has(key);
      }
      async listKeys(address, prefix = "") {
        return [...this.#space(address).keys()].filter((k) => k.startsWith(prefix));
      }
      async clear(address, prefix = "") {
        const space = this.#space(address);
        if (!prefix) return space.clear();
        for (const k of [...space.keys()]) if (k.startsWith(prefix)) space.delete(k);
      }
    };
    enc = new TextEncoder();
    CRLF = "\r\n";
    CODING_FORMATS = [
      ["zstd", ["zstd"]],
      ["br", ["br", "brotli"]],
      ["gzip", ["gzip"]],
      ["deflate", ["deflate"]]
      // HTTP "deflate" is the zlib format, as is the API's
    ];
    kpsResolverKind = {
      matches: (url) => url.startsWith("kps:"),
      fetch: (url, maxBytes) => {
        const { addr, path } = parseKpsResolver(url);
        return fetchBundleOverKps(addr, path, maxBytes);
      }
    };
    LOG_RETENTION = 1e3;
    AnonRpcWorker = class {
      ready;
      fetch;
      #init;
      #config;
      #rpc;
      #iframe;
      #disposeKps;
      #onWindowMessage;
      // Calls awaiting acceptCall (§8: ordered, no drop, backpressured).
      #queue = new CallQueue();
      // Log entries awaiting acceptLog (§13.1: ordered, bounded, lossy).
      #logs = new LogQueue(LOG_RETENTION);
      // Set once the host has shown it is collecting logs, which is what stops
      // them also going to the console. Not a setting: a host that reads its
      // worker's logs does not want them duplicated into a console it is not
      // looking at, and one that never reads still gets the old behaviour.
      #logsClaimed = false;
      #calls = /* @__PURE__ */ new Map();
      #nextCallId = 1;
      #readyResolve;
      #readyReject;
      // Set on fatal failure (boot failure, worker crash, close): everything
      // pending is rejected with it and future fetches fail fast.
      #failure;
      // §11 storage backend (IndexedDB; memory fallback), opened on first use.
      #storage;
      constructor(init) {
        this.#init = init;
        this.#config = copyPlainValue(init.config, "config");
        this.ready = new Promise((res, rej) => {
          this.#readyResolve = res;
          this.#readyReject = rej;
        });
        this.ready.catch(() => {
        });
        this.fetch = this.#fetch.bind(this);
        this.#boot().catch((e) => this.#fail(e));
      }
      // A dead worker fails everything, not just `ready`: in-flight and queued
      // fetches would otherwise hang with no error surfaced.
      #fail(err) {
        if (this.#failure !== void 0) return;
        this.#failure = err;
        this.#readyReject(err);
        for (const rec of this.#calls.values()) {
          rec.cleanup?.();
          rec.reject(err);
        }
        this.#calls.clear();
        this.#queue.rejectAll(err);
        this.#logs.close(err);
      }
      async #boot() {
        const provider = this.#init.preExisting?.rpcProvider;
        if (!provider) throw new Error("preExisting.rpcProvider is required to read the specifier");
        const spec = await readSpecifier(provider, this.#init.address);
        const bundleBytes = await fetchAndVerifyBundle(spec, MAX_BUNDLE_BYTES, [kpsResolverKind]);
        const channel = new MessageChannel();
        const rpc = new PortRpc(channel.port1);
        this.#rpc = rpc;
        this.#registerHandlers(rpc);
        const iframe = document.createElement("iframe");
        iframe.setAttribute("sandbox", "allow-scripts");
        iframe.style.display = "none";
        if (this.#init.iframeUrl) iframe.src = sameOriginIframeUrl(this.#init.iframeUrl);
        else iframe.srcdoc = `<!doctype html><meta charset="utf-8"><script>${'"use strict";\n(() => {\n  // src/iframe/iframe-boot.ts\n  (() => {\n    parent.postMessage({ kind: "iframe-ready" }, "*");\n    const onInit = (ev) => {\n      if (ev.source !== parent || ev.data?.kind !== "init") return;\n      removeEventListener("message", onInit);\n      const port = ev.ports[0];\n      const runtimeSource = ev.data.runtimeSource;\n      const bundleBytes = ev.data.bundleBytes;\n      const blob = new Blob([runtimeSource], { type: "text/javascript" });\n      const worker = new Worker(URL.createObjectURL(blob));\n      worker.postMessage({ kind: "init", port, bundleBytes, config: ev.data.config }, [port]);\n      worker.addEventListener("error", (e) => {\n        parent.postMessage({ kind: "worker-error", message: e.message }, "*");\n      });\n    };\n    addEventListener("message", onInit);\n  })();\n})();\n'}<\/script>`;
        this.#iframe = iframe;
        const ready = new Promise((resolve) => {
          const onMessage = (ev) => {
            if (ev.source !== iframe.contentWindow) return;
            if (ev.data?.kind === "iframe-ready") resolve();
            if (ev.data?.kind === "worker-error") {
              this.#fail(new Error(`worker error: ${ev.data.message}`));
            }
          };
          this.#onWindowMessage = onMessage;
          window.addEventListener("message", onMessage);
        });
        document.body.appendChild(iframe);
        await ready;
        iframe.contentWindow.postMessage(
          { kind: "init", runtimeSource: '"use strict";\n(() => {\n  // ../harness-core/dist/errors.js\n  var RpcError = class extends Error {\n    code;\n    constructor(e) {\n      super(e.message);\n      this.name = e.name;\n      this.code = e.code;\n    }\n  };\n  function serializeError(err) {\n    if (err instanceof Error) {\n      const code = err.code;\n      return {\n        name: err.name,\n        message: err.message,\n        ...typeof code === "string" ? { code } : {}\n      };\n    }\n    return { name: "Error", message: String(err) };\n  }\n\n  // src/protocol.ts\n  var PortRpc = class {\n    #port;\n    #nextId = 1;\n    #pending = /* @__PURE__ */ new Map();\n    #handlers = /* @__PURE__ */ new Map();\n    #events = /* @__PURE__ */ new Map();\n    // AbortControllers for in-flight inbound requests, keyed by caller-side id.\n    #inbound = /* @__PURE__ */ new Map();\n    constructor(port) {\n      this.#port = port;\n      port.onmessage = (ev) => this.#onMessage(ev.data);\n      port.start?.();\n    }\n    /** Register a handler for an inbound method. */\n    on(method, handler) {\n      this.#handlers.set(method, handler);\n    }\n    /** Register a handler for an inbound fire-and-forget event. */\n    onEvent(topic, handler) {\n      this.#events.set(topic, handler);\n    }\n    /** Fire-and-forget event to the peer. */\n    emit(topic, data, transfer) {\n      this.#post({ t: "evt", topic, data }, transfer);\n    }\n    /** Call a remote method and await its result. */\n    call(method, args, opts) {\n      const id = this.#nextId++;\n      return new Promise((resolve, reject) => {\n        let cleanup;\n        if (opts?.signal) {\n          const signal = opts.signal;\n          if (signal.aborted) {\n            reject(abortError());\n            return;\n          }\n          const onAbort = () => {\n            if (!this.#pending.delete(id)) return;\n            this.#post({ t: "abort", id });\n            reject(abortError());\n          };\n          signal.addEventListener("abort", onAbort);\n          cleanup = () => signal.removeEventListener("abort", onAbort);\n        }\n        this.#pending.set(id, {\n          resolve,\n          reject,\n          cleanup\n        });\n        this.#post({ t: "req", id, method, args }, opts?.transfer);\n      });\n    }\n    #post(msg, transfer) {\n      this.#port.postMessage(msg, transfer ?? []);\n    }\n    async #onMessage(msg) {\n      switch (msg.t) {\n        case "evt": {\n          this.#events.get(msg.topic)?.(msg.data);\n          return;\n        }\n        case "abort": {\n          this.#inbound.get(msg.id)?.abort(abortError());\n          return;\n        }\n        case "res": {\n          const p = this.#pending.get(msg.id);\n          if (!p) return;\n          this.#pending.delete(msg.id);\n          p.cleanup?.();\n          if (msg.ok) p.resolve(msg.value);\n          else p.reject(new RpcError(msg.error));\n          return;\n        }\n        case "req": {\n          const handler = this.#handlers.get(msg.method);\n          if (!handler) {\n            this.#post({\n              t: "res",\n              id: msg.id,\n              ok: false,\n              error: { name: "Error", message: `no handler: ${msg.method}` }\n            });\n            return;\n          }\n          const ac = new AbortController();\n          this.#inbound.set(msg.id, ac);\n          try {\n            const result = await handler(msg.args, { signal: ac.signal });\n            this.#post(\n              { t: "res", id: msg.id, ok: true, value: result.value },\n              result.transfer\n            );\n          } catch (err) {\n            this.#post({\n              t: "res",\n              id: msg.id,\n              ok: false,\n              error: serializeError(err)\n            });\n          } finally {\n            this.#inbound.delete(msg.id);\n          }\n          return;\n        }\n      }\n    }\n  };\n  function abortError() {\n    return new DOMException("The operation was aborted.", "AbortError");\n  }\n\n  // src/worker/anon-rpc-worker-api.ts\n  function makeWorkerApi(rpc, config) {\n    const callAborts = /* @__PURE__ */ new Map();\n    const preAborted = /* @__PURE__ */ new Set();\n    rpc.onEvent("call-abort", ({ callId }) => {\n      const ac = callAborts.get(callId);\n      if (ac) ac.abort(new DOMException("aborted", "AbortError"));\n      else preAborted.add(callId);\n    });\n    const streamClosed = /* @__PURE__ */ new Map();\n    const connClosed = /* @__PURE__ */ new Map();\n    rpc.onEvent("stream.closed", ({ streamId, info }) => {\n      streamClosed.get(streamId)?.(info);\n      streamClosed.delete(streamId);\n    });\n    rpc.onEvent("conn.closed", ({ connId, info }) => {\n      connClosed.get(connId)?.(info);\n      connClosed.delete(connId);\n    });\n    function makeStream(p) {\n      const closed = new Promise((resolve) => streamClosed.set(p.streamId, resolve));\n      return {\n        readable: p.readable,\n        writable: p.writable,\n        closeWrite: () => rpc.call("stream.closeWrite", { streamId: p.streamId }),\n        cancelRead: (reason) => rpc.call("stream.cancelRead", { streamId: p.streamId, reason }),\n        resetWrite: (reason) => rpc.call("stream.resetWrite", { streamId: p.streamId, reason }),\n        close: (reason) => rpc.call("stream.close", { streamId: p.streamId, reason }),\n        closed\n      };\n    }\n    function makeConn(connId, remoteAddress) {\n      const closed = new Promise((resolve) => connClosed.set(connId, resolve));\n      return {\n        remoteAddress,\n        openStream: async (opts) => makeStream(await rpc.call("conn.openStream", { connId }, sigOpt(opts))),\n        acceptStream: async (opts) => makeStream(await rpc.call("conn.acceptStream", { connId }, sigOpt(opts))),\n        close: (reason) => rpc.call("conn.close", { connId, reason }),\n        sendDatagram: (data, opts) => rpc.call("conn.sendDatagram", { connId, data }, sigOpt(opts)),\n        receiveDatagram: (opts) => rpc.call("conn.receiveDatagram", { connId }, sigOpt(opts)),\n        closed\n      };\n    }\n    const kps = {\n      dial: async (addr, opts) => {\n        const { connId, remoteAddress } = await rpc.call("kps.dial", { addr }, sigOpt(opts));\n        return makeConn(connId, remoteAddress);\n      },\n      openStream: async (addr, opts) => makeStream(await rpc.call("kps.openStream", { addr }, sigOpt(opts)))\n    };\n    const storage = {\n      get: (key, opts) => rpc.call("storage.get", { key }, sigOpt(opts)),\n      set: (key, value, opts) => rpc.call("storage.set", { key, value }, sigOpt(opts)),\n      delete: (key, opts) => rpc.call("storage.delete", { key }, sigOpt(opts)),\n      has: (key, opts) => rpc.call("storage.has", { key }, sigOpt(opts)),\n      clear: (opts) => rpc.call("storage.clear", { prefix: opts?.prefix }, sigOpt(opts)),\n      list: (opts) => listKeys(rpc, opts?.prefix, opts?.signal)\n    };\n    const log = {\n      debug: (...args) => rpc.emit("log", { level: "debug", args }),\n      info: (...args) => rpc.emit("log", { level: "info", args }),\n      warn: (...args) => rpc.emit("log", { level: "warn", args }),\n      error: (...args) => rpc.emit("log", { level: "error", args })\n    };\n    function acceptCall(opts) {\n      const abortErr = () => new DOMException("aborted", "AbortError");\n      const signal = opts?.signal;\n      const toCall = (r) => makeFetchCall(rpc, callAborts, preAborted, r.callId, r.url, r.requestInit);\n      if (signal?.aborted) return Promise.reject(signal.reason ?? abortErr());\n      const pending = rpc.call("acceptCall", {});\n      if (!signal) return pending.then(toCall);\n      return new Promise((resolve, reject) => {\n        let settled = false;\n        const onAbort = () => {\n          if (settled) return;\n          settled = true;\n          pending.then((r) => rpc.emit("requeue-call", { callId: r.callId }), () => {\n          });\n          reject(signal.reason ?? abortErr());\n        };\n        signal.addEventListener("abort", onAbort, { once: true });\n        pending.then(\n          (r) => {\n            if (settled) return;\n            settled = true;\n            signal.removeEventListener("abort", onAbort);\n            resolve(toCall(r));\n          },\n          (e) => {\n            if (settled) return;\n            settled = true;\n            signal.removeEventListener("abort", onAbort);\n            reject(e);\n          }\n        );\n      });\n    }\n    return {\n      signalReady: () => rpc.emit("ready", {}),\n      // Failure finality (\xA77: later signals ignored) is enforced host-side \u2014\n      // #fail is one-shot and resolving an already-rejected ready is a no-op.\n      signalFailed: (reason) => rpc.emit("failed", { reason }),\n      acceptCall,\n      config,\n      kps,\n      storage,\n      log\n    };\n  }\n  function makeFetchCall(rpc, callAborts, preAborted, callId, url, wireInit) {\n    let requestInit;\n    if (wireInit) {\n      const { hasSignal, forwarded, ...rest } = wireInit;\n      requestInit = { ...forwarded, ...rest };\n      if (hasSignal) {\n        const ac = new AbortController();\n        callAborts.set(callId, ac);\n        requestInit = { ...requestInit, signal: ac.signal };\n        if (preAborted.delete(callId)) ac.abort(new DOMException("aborted", "AbortError"));\n      }\n    }\n    let responded = false;\n    return {\n      kind: "fetch",\n      url,\n      ...requestInit ? { requestInit } : {},\n      respond(response) {\n        if (responded) throw new Error("respond() called more than once");\n        responded = true;\n        Promise.resolve(response).then((r) => {\n          const transfer = r.body instanceof ReadableStream ? [r.body] : r.body instanceof Uint8Array ? [r.body.buffer] : [];\n          return rpc.call("respond", { callId, ok: true, response: r }, { transfer });\n        }).catch(\n          (e) => (\n            // Carry name/code across the boundary (\xA712): host logic may branch\n            // on code, never on message text.\n            rpc.call("respond", { callId, ok: false, error: serializeError(e) })\n          )\n        ).catch(() => {\n        }).finally(() => callAborts.delete(callId));\n      }\n    };\n  }\n  async function* listKeys(rpc, prefix, signal) {\n    const keys = await rpc.call("storage.list", { prefix }, signal ? { signal } : void 0);\n    for (const k of keys) yield k;\n  }\n  function sigOpt(opts) {\n    return opts?.signal ? { signal: opts.signal } : void 0;\n  }\n\n  // src/worker/worker-runtime.ts\n  var importScripts = (url) => (\n    // eslint-disable-next-line no-restricted-globals\n    self.importScripts(url)\n  );\n  function bundleScriptUrl(bundle) {\n    const probeUrl = URL.createObjectURL(new Blob([";"], { type: "text/javascript" }));\n    try {\n      importScripts(probeUrl);\n      return URL.createObjectURL(bundle);\n    } catch {\n      return new FileReaderSync().readAsDataURL(bundle);\n    } finally {\n      URL.revokeObjectURL(probeUrl);\n    }\n  }\n  addEventListener("message", function onInit(ev) {\n    if (ev.data?.kind !== "init") return;\n    removeEventListener("message", onInit);\n    const port = ev.data.port;\n    const bundleBytes = ev.data.bundleBytes;\n    const config = ev.data.config;\n    const rpc = new PortRpc(port);\n    const api = makeWorkerApi(rpc, config);\n    globalThis.anonRpcWorker = api;\n    try {\n      const blob = new Blob([bundleBytes], { type: "text/javascript" });\n      importScripts(bundleScriptUrl(blob));\n    } catch (err) {\n      const message = err?.message ?? String(err);\n      api.log.error("worker bundle failed to load:", message);\n      rpc.emit("boot-error", { message });\n    }\n  });\n})();\n', bundleBytes, config: this.#config },
          "*",
          [channel.port2]
        );
      }
      #registerHandlers(rpc) {
        this.#disposeKps = registerKpsBridge(rpc);
        rpc.onEvent("ready", () => this.#readyResolve());
        rpc.onEvent("boot-error", ({ message }) => {
          this.#fail(new Error(`worker bundle failed to load: ${message}`));
        });
        rpc.onEvent("failed", ({ reason }) => {
          this.#fail(
            new RpcError({
              name: "WorkerFailedError",
              message: reason?.message ?? "worker signalled failure",
              code: reason?.code
            })
          );
        });
        rpc.onEvent("requeue-call", ({ callId }) => {
          const rec = this.#calls.get(callId);
          if (rec) this.#queue.pushFront(rec);
        });
        rpc.onEvent("log", ({ level, args }) => {
          this.#logs.push({ level, args });
          if (this.#logsClaimed) return;
          const fn = console[level] ?? console.log;
          fn.call(console, "[worker]", ...args.map(renderLogArg));
        });
        rpc.on("acceptCall", async (_args, { signal }) => {
          const rec = await this.#queue.take(signal);
          const value = {
            callId: rec.callId,
            url: rec.url,
            requestInit: rec.requestInit
          };
          return { value, transfer: rec.bodyTransfer ?? [] };
        });
        rpc.on("respond", async (msg) => {
          const rec = this.#calls.get(msg.callId);
          if (!rec) return { value: void 0 };
          this.#calls.delete(msg.callId);
          rec.cleanup?.();
          if (msg.ok && msg.response) {
            try {
              rec.resolve(toResponse(msg.response));
            } catch (e) {
              rec.reject(e);
            }
          } else {
            rec.reject(new RpcError(msg.error ?? { name: "Error", message: "worker failed the call" }));
          }
          return { value: void 0 };
        });
        const addr = this.#init.address.toLowerCase();
        const store = () => this.#storage ??= openStorageBackend();
        rpc.on("storage.get", async ({ key }) => ({ value: await (await store()).get(addr, key) }));
        rpc.on("storage.set", async ({ key, value }) => {
          await (await store()).set(addr, key, value);
          return { value: void 0 };
        });
        rpc.on("storage.delete", async ({ key }) => {
          await (await store()).delete(addr, key);
          return { value: void 0 };
        });
        rpc.on("storage.has", async ({ key }) => ({ value: await (await store()).has(addr, key) }));
        rpc.on("storage.list", async ({ prefix }) => ({
          value: await (await store()).listKeys(addr, prefix)
        }));
        rpc.on("storage.clear", async ({ prefix }) => {
          await (await store()).clear(addr, prefix);
          return { value: void 0 };
        });
      }
      async #fetch(input, init) {
        if (this.#failure !== void 0) throw this.#failure;
        const { url, requestInit, transfer, signal: hostSignal } = await normalizeRequest(input, init);
        const callId = this.#nextCallId++;
        return new Promise((resolve, reject) => {
          const rec = {
            callId,
            url,
            requestInit,
            bodyTransfer: transfer,
            resolve,
            reject
          };
          this.#calls.set(callId, rec);
          if (hostSignal) {
            const onAbort = () => {
              if (!this.#queue.remove(rec)) this.#rpc?.emit("call-abort", { callId });
              if (this.#calls.delete(callId)) {
                reject(hostSignal.reason ?? new DOMException("aborted", "AbortError"));
              }
            };
            if (hostSignal.aborted) {
              this.#calls.delete(callId);
              reject(hostSignal.reason ?? new DOMException("aborted", "AbortError"));
              return;
            }
            hostSignal.addEventListener("abort", onAbort, { once: true });
            rec.cleanup = () => hostSignal.removeEventListener("abort", onAbort);
          }
          this.#queue.push(rec);
        });
      }
      /**
       * §5/§13.1: the next log entry the worker produced, waiting for one if the
       * buffer is empty.
       *
       * Calling it at all is what claims the worker's logs: from the first call,
       * entries stop being echoed to the console, because a host that collects
       * them is not served by seeing them twice.
       */
      acceptLog(opts) {
        this.#logsClaimed = true;
        return this.#logs.take(opts?.signal);
      }
      close() {
        this.#disposeKps?.();
        if (this.#onWindowMessage) window.removeEventListener("message", this.#onWindowMessage);
        this.#iframe?.remove();
        this.#fail(new Error("worker closed"));
      }
    };
  }
});

// node_modules/viem/_esm/index.js
init_abis();
init_decodeFunctionResult();
init_encodeFunctionData();
init_toHex();

// ../../../vendor/kohaku/packages/privacy-pools/src/data/abis/entrypoint.abi.ts
var entrypointAbi = [
  {
    type: "function",
    name: "relay",
    inputs: [
      {
        name: "_withdrawal",
        type: "tuple",
        internalType: "struct IPrivacyPool.Withdrawal",
        components: [
          {
            name: "processooor",
            type: "address",
            internalType: "address"
          },
          {
            name: "data",
            type: "bytes",
            internalType: "bytes"
          }
        ]
      },
      {
        name: "_proof",
        type: "tuple",
        internalType: "struct ProofLib.WithdrawProof",
        components: [
          {
            name: "pA",
            type: "uint256[2]",
            internalType: "uint256[2]"
          },
          {
            name: "pB",
            type: "uint256[2][2]",
            internalType: "uint256[2][2]"
          },
          {
            name: "pC",
            type: "uint256[2]",
            internalType: "uint256[2]"
          },
          {
            name: "pubSignals",
            type: "uint256[8]",
            internalType: "uint256[8]"
          }
        ]
      },
      {
        name: "_scope",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    outputs: [],
    stateMutability: "nonpayable"
  },
  {
    type: "function",
    name: "latestRoot",
    inputs: [],
    outputs: [
      {
        name: "",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    stateMutability: "view"
  },
  {
    type: "function",
    name: "rootByIndex",
    inputs: [
      {
        name: "_index",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    outputs: [
      {
        name: "",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    stateMutability: "view"
  },
  {
    type: "function",
    name: "assetConfig",
    inputs: [
      {
        name: "_asset",
        type: "address",
        internalType: "contract IERC20"
      }
    ],
    outputs: [
      {
        name: "pool",
        type: "address",
        internalType: "contract IPrivacyPool"
      },
      {
        name: "minimumDepositAmount",
        type: "uint256",
        internalType: "uint256"
      },
      {
        name: "vettingFeeBPS",
        type: "uint256",
        internalType: "uint256"
      },
      {
        name: "maxRelayFeeBPS",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    stateMutability: "view"
  }
];

// ../../../vendor/kohaku/packages/privacy-pools/src/data/abis/paymaster.abi.ts
var paymasterAbi = [
  {
    type: "function",
    name: "quoteWeiInToken",
    stateMutability: "view",
    inputs: [
      { name: "feeToken", type: "address" },
      { name: "weiAmount", type: "uint256" }
    ],
    outputs: [{ name: "tokenAmount", type: "uint256" }]
  }
];

// ../../../vendor/kohaku/packages/privacy-pools/src/data/abis/pool.abi.ts
var poolAbi = [
  {
    "type": "function",
    "name": "ASSET",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "address",
        "internalType": "address"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "SCOPE",
    "inputs": [],
    "outputs": [
      {
        "name": "_scope",
        "type": "uint256",
        "internalType": "uint256"
      }
    ],
    "stateMutability": "view"
  },
  {
    type: "function",
    name: "currentRoot",
    inputs: [],
    outputs: [
      {
        name: "",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    stateMutability: "view"
  },
  {
    type: "function",
    name: "currentRootIndex",
    inputs: [],
    outputs: [
      {
        name: "",
        type: "uint32",
        internalType: "uint32"
      }
    ],
    stateMutability: "view"
  },
  {
    type: "function",
    name: "roots",
    inputs: [
      {
        name: "_index",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    outputs: [
      {
        name: "",
        type: "uint256",
        internalType: "uint256"
      }
    ],
    stateMutability: "view"
  },
  {
    type: "function",
    name: "ragequit",
    inputs: [
      {
        name: "_proof",
        type: "tuple",
        internalType: "struct ProofLib.RagequitProof",
        components: [
          {
            name: "pA",
            type: "uint256[2]",
            internalType: "uint256[2]"
          },
          {
            name: "pB",
            type: "uint256[2][2]",
            internalType: "uint256[2][2]"
          },
          {
            name: "pC",
            type: "uint256[2]",
            internalType: "uint256[2]"
          },
          {
            name: "pubSignals",
            type: "uint256[4]",
            internalType: "uint256[4]"
          }
        ]
      }
    ],
    outputs: [],
    stateMutability: "nonpayable"
  },
  {
    type: "function",
    name: "withdraw",
    inputs: [
      {
        name: "_withdrawal",
        type: "tuple",
        internalType: "struct IPrivacyPool.Withdrawal",
        components: [
          { name: "processooor", type: "address", internalType: "address" },
          { name: "data", type: "bytes", internalType: "bytes" }
        ]
      },
      {
        name: "_proof",
        type: "tuple",
        internalType: "struct ProofLib.WithdrawProof",
        components: [
          { name: "pA", type: "uint256[2]", internalType: "uint256[2]" },
          { name: "pB", type: "uint256[2][2]", internalType: "uint256[2][2]" },
          { name: "pC", type: "uint256[2]", internalType: "uint256[2]" },
          { name: "pubSignals", type: "uint256[8]", internalType: "uint256[8]" }
        ]
      }
    ],
    outputs: [],
    stateMutability: "nonpayable"
  }
];

// ../../../vendor/kohaku/packages/privacy-pools/src/data/eth-client.ts
var abis = {
  erc20: erc20Abi,
  pool: poolAbi,
  entrypoint: entrypointAbi,
  paymaster: paymasterAbi
};
var EthClient = class {
  constructor(provider) {
    this.provider = provider;
  }
  provider;
  request = (...params) => {
    return this.provider.request(...params);
  };
  async getChainId() {
    return this.provider.getChainId();
  }
  async getLogs({
    maxQuerySize = 5000n,
    ...params
  }) {
    const fromBlock = params.fromBlock;
    const toBlock = params.toBlock ?? await this.provider.getBlockNumber();
    const logs = [];
    for (let start = fromBlock; start <= toBlock; start += maxQuerySize) {
      const rawEnd = start + maxQuerySize;
      const end = rawEnd < toBlock ? rawEnd : toBlock;
      const result = await this.provider.getLogs({
        address: params.address,
        fromBlock: start,
        toBlock: end
      });
      logs.push(...result);
    }
    return logs;
  }
  async getCode(address) {
    return this.provider.getCode(address);
  }
  async getBlockNumber() {
    return this.provider.getBlockNumber();
  }
  async makeContractRequest(contractAddress, contractName, functionName, ...args) {
    const abi = abis[contractName];
    const data = encodeFunctionData({
      abi,
      functionName,
      args
    });
    const result = await this.request({
      method: "eth_call",
      params: [{
        to: toHex(contractAddress, { size: 20 }),
        data
      }, "latest"]
    });
    return decodeFunctionResult({
      abi,
      functionName,
      data: result
    });
  }
};

// ../../../vendor/kohaku/packages/provider/src/raw/hex.ts
var assertHexQuantity = (value) => {
  if (!/^0x[0-9a-fA-F]+$/.test(value)) {
    throw new Error(`Expected hex quantity, got: ${String(value)}`);
  }
};
var hexToBigInt2 = (value) => {
  assertHexQuantity(value);
  return BigInt(value);
};

// node_modules/ox/_esm/core/version.js
var version2 = "0.1.1";

// node_modules/ox/_esm/core/internal/errors.js
function getVersion() {
  return version2;
}

// node_modules/ox/_esm/core/Errors.js
var BaseError2 = class _BaseError extends Error {
  static setStaticOptions(options) {
    _BaseError.prototype.docsOrigin = options.docsOrigin;
    _BaseError.prototype.showVersion = options.showVersion;
    _BaseError.prototype.version = options.version;
  }
  constructor(shortMessage, options = {}) {
    const details = (() => {
      if (options.cause instanceof _BaseError) {
        if (options.cause.details)
          return options.cause.details;
        if (options.cause.shortMessage)
          return options.cause.shortMessage;
      }
      if (options.cause && "details" in options.cause && typeof options.cause.details === "string")
        return options.cause.details;
      if (options.cause?.message)
        return options.cause.message;
      return options.details;
    })();
    const docsPath3 = (() => {
      if (options.cause instanceof _BaseError)
        return options.cause.docsPath || options.docsPath;
      return options.docsPath;
    })();
    const docsBaseUrl = options.docsOrigin ?? _BaseError.prototype.docsOrigin;
    const docs = `${docsBaseUrl}${docsPath3 ?? ""}`;
    const showVersion = Boolean(options.version ?? _BaseError.prototype.showVersion);
    const version3 = options.version ?? _BaseError.prototype.version;
    const message = [
      shortMessage || "An error occurred.",
      ...options.metaMessages ? ["", ...options.metaMessages] : [],
      ...details || docsPath3 || showVersion ? [
        "",
        details ? `Details: ${details}` : void 0,
        docsPath3 ? `See: ${docs}` : void 0,
        showVersion ? `Version: ${version3}` : void 0
      ] : []
    ].filter((x) => typeof x === "string").join("\n");
    super(message, options.cause ? { cause: options.cause } : void 0);
    Object.defineProperty(this, "details", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "docs", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "docsOrigin", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "docsPath", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "shortMessage", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "showVersion", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "version", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "cause", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: void 0
    });
    Object.defineProperty(this, "name", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: "BaseError"
    });
    this.cause = options.cause;
    this.details = details;
    this.docs = docs;
    this.docsOrigin = docsBaseUrl;
    this.docsPath = docsPath3;
    this.shortMessage = shortMessage;
    this.showVersion = showVersion;
    this.version = version3;
  }
  walk(fn) {
    return walk2(this, fn);
  }
};
Object.defineProperty(BaseError2, "defaultStaticOptions", {
  enumerable: true,
  configurable: true,
  writable: true,
  value: {
    docsOrigin: "https://oxlib.sh",
    showVersion: false,
    version: `ox@${getVersion()}`
  }
});
(() => {
  BaseError2.setStaticOptions(BaseError2.defaultStaticOptions);
})();
function walk2(err, fn) {
  if (fn?.(err))
    return err;
  if (err && typeof err === "object" && "cause" in err && err.cause)
    return walk2(err.cause, fn);
  return fn ? null : err;
}

// node_modules/ox/_esm/core/internal/hex.js
function pad2(hex_, options = {}) {
  const { dir, size: size3 = 32 } = options;
  if (size3 === 0)
    return hex_;
  const hex = hex_.replace("0x", "");
  if (hex.length > size3 * 2)
    throw new SizeExceedsPaddingSizeError2({
      size: Math.ceil(hex.length / 2),
      targetSize: size3,
      type: "Hex"
    });
  return `0x${hex[dir === "right" ? "padEnd" : "padStart"](size3 * 2, "0")}`;
}

// node_modules/ox/_esm/core/Json.js
var bigIntSuffix = "#__bigint";
function stringify(value, replacer, space) {
  return JSON.stringify(value, (key, value2) => {
    if (typeof replacer === "function")
      return replacer(key, value2);
    if (typeof value2 === "bigint")
      return value2.toString() + bigIntSuffix;
    return value2;
  }, space);
}

// node_modules/ox/_esm/core/Hex.js
function assert(value, options = {}) {
  const { strict = false } = options;
  if (!value)
    throw new InvalidHexTypeError(value);
  if (typeof value !== "string")
    throw new InvalidHexTypeError(value);
  if (strict) {
    if (!/^0x[0-9a-fA-F]*$/.test(value))
      throw new InvalidHexValueError(value);
  }
  if (!value.startsWith("0x"))
    throw new InvalidHexValueError(value);
}
function fromNumber(value, options = {}) {
  const { signed, size: size3 } = options;
  const value_ = BigInt(value);
  let maxValue;
  if (size3) {
    if (signed)
      maxValue = (1n << BigInt(size3) * 8n - 1n) - 1n;
    else
      maxValue = 2n ** (BigInt(size3) * 8n) - 1n;
  } else if (typeof value === "number") {
    maxValue = BigInt(Number.MAX_SAFE_INTEGER);
  }
  const minValue = typeof maxValue === "bigint" && signed ? -maxValue - 1n : 0;
  if (maxValue && value_ > maxValue || value_ < minValue) {
    const suffix = typeof value === "bigint" ? "n" : "";
    throw new IntegerOutOfRangeError2({
      max: maxValue ? `${maxValue}${suffix}` : void 0,
      min: `${minValue}${suffix}`,
      signed,
      size: size3,
      value: `${value}${suffix}`
    });
  }
  const stringValue = (signed && value_ < 0 ? BigInt.asUintN(size3 * 8, BigInt(value_)) : value_).toString(16);
  const hex = `0x${stringValue}`;
  if (size3)
    return padLeft(hex, size3);
  return hex;
}
function padLeft(value, size3) {
  return pad2(value, { dir: "left", size: size3 });
}
function validate(value, options = {}) {
  const { strict = false } = options;
  try {
    assert(value, { strict });
    return true;
  } catch {
    return false;
  }
}
var IntegerOutOfRangeError2 = class extends BaseError2 {
  constructor({ max, min, signed, size: size3, value }) {
    super(`Number \`${value}\` is not in safe${size3 ? ` ${size3 * 8}-bit` : ""}${signed ? " signed" : " unsigned"} integer range ${max ? `(\`${min}\` to \`${max}\`)` : `(above \`${min}\`)`}`);
    Object.defineProperty(this, "name", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: "Hex.IntegerOutOfRangeError"
    });
  }
};
var InvalidHexTypeError = class extends BaseError2 {
  constructor(value) {
    super(`Value \`${typeof value === "object" ? stringify(value) : value}\` of type \`${typeof value}\` is an invalid hex type.`, {
      metaMessages: ['Hex types must be represented as `"0x${string}"`.']
    });
    Object.defineProperty(this, "name", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: "Hex.InvalidHexTypeError"
    });
  }
};
var InvalidHexValueError = class extends BaseError2 {
  constructor(value) {
    super(`Value \`${value}\` is an invalid hex value.`, {
      metaMessages: [
        'Hex values must start with `"0x"` and contain only hexadecimal characters (0-9, a-f, A-F).'
      ]
    });
    Object.defineProperty(this, "name", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: "Hex.InvalidHexValueError"
    });
  }
};
var SizeExceedsPaddingSizeError2 = class extends BaseError2 {
  constructor({ size: size3, targetSize, type }) {
    super(`${type.charAt(0).toUpperCase()}${type.slice(1).toLowerCase()} size (\`${size3}\`) exceeds padding size (\`${targetSize}\`).`);
    Object.defineProperty(this, "name", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: "Hex.SizeExceedsPaddingSizeError"
    });
  }
};

// node_modules/ox/_esm/core/Filter.js
var Filter_exports = {};
__export(Filter_exports, {
  fromRpc: () => fromRpc,
  toRpc: () => toRpc
});
function fromRpc(filter) {
  const { fromBlock, toBlock } = filter;
  return {
    ...filter,
    ...fromBlock && {
      fromBlock: validate(fromBlock, { strict: false }) ? BigInt(fromBlock) : fromBlock
    },
    ...toBlock && {
      toBlock: validate(toBlock, { strict: false }) ? BigInt(toBlock) : toBlock
    }
  };
}
function toRpc(filter) {
  const { address, topics, fromBlock, toBlock } = filter;
  return {
    ...address && { address },
    ...topics && { topics },
    ...typeof fromBlock !== "undefined" ? {
      fromBlock: typeof fromBlock === "bigint" ? fromNumber(fromBlock) : fromBlock
    } : {},
    ...typeof toBlock !== "undefined" ? {
      toBlock: typeof toBlock === "bigint" ? fromNumber(toBlock) : toBlock
    } : {}
  };
}

// ../../../vendor/kohaku/packages/provider/src/raw/wait.ts
var DEFAULT_WAIT_FOR_TRANSACTION = {
  pollIntervalMs: 100,
  timeoutMs: 1e4
};
var resolveWaitForTransactionOptions = (options = {}) => {
  const resolved = {
    pollIntervalMs: options.pollIntervalMs ?? DEFAULT_WAIT_FOR_TRANSACTION.pollIntervalMs,
    timeoutMs: options.timeoutMs ?? DEFAULT_WAIT_FOR_TRANSACTION.timeoutMs
  };
  for (const [name, value] of Object.entries(resolved)) {
    if (!Number.isInteger(value) || value <= 0 || value > 2147483647) {
      throw new RangeError(`${name} must be an integer between 1 and 2147483647 milliseconds`);
    }
  }
  return resolved;
};
var waitForReceipt = async (txHash, getReceipt, options) => {
  const started = performance.now();
  const timeoutError = () => new Error(`Timed out waiting for transaction: ${txHash}`);
  let stopped = false;
  let timeoutTimer;
  let pollTimer;
  const timeout = new Promise((_, reject) => {
    timeoutTimer = setTimeout(() => {
      stopped = true;
      reject(timeoutError());
    }, options.timeoutMs);
  });
  const poll = async () => {
    while (!stopped) {
      const receipt = await getReceipt();
      if (stopped) return;
      if (performance.now() - started >= options.timeoutMs) throw timeoutError();
      if (receipt) return;
      await new Promise((resolve) => {
        pollTimer = setTimeout(resolve, options.pollIntervalMs);
      });
    }
  };
  try {
    await Promise.race([poll(), timeout]);
  } finally {
    stopped = true;
    clearTimeout(timeoutTimer);
    clearTimeout(pollTimer);
  }
};

// ../../../vendor/kohaku/packages/provider/src/raw/index.ts
var raw = (client, options = {}) => {
  const waitOptions = resolveWaitForTransactionOptions(options.waitForTransaction);
  const getTransactionReceipt = async (txHash) => {
    const receipt = await client.request({
      method: "eth_getTransactionReceipt",
      params: [txHash]
    });
    if (!receipt) return null;
    return convertReceipt(receipt);
  };
  return {
    _internal: client,
    request: client.request.bind(client),
    async getLogs(params) {
      const logs = await client.request({
        method: "eth_getLogs",
        params: [Filter_exports.toRpc(params)]
      });
      return logs.map(convertLog);
    },
    async getChainId() {
      const hex = await client.request({
        method: "eth_chainId",
        params: void 0
      });
      return hexToBigInt2(hex);
    },
    async getBlockNumber() {
      const hex = await client.request({
        method: "eth_blockNumber",
        params: void 0
      });
      return hexToBigInt2(hex);
    },
    waitForTransaction: (txHash) => waitForReceipt(txHash, () => getTransactionReceipt(txHash), waitOptions),
    async getBalance(address, block) {
      const hex = await client.request({
        method: "eth_getBalance",
        params: [address, block ?? "latest"]
      });
      return hexToBigInt2(hex);
    },
    async getCode(address, block) {
      const hex = await client.request({
        method: "eth_getCode",
        params: [address, block ?? "latest"]
      });
      return hex ?? "0x";
    },
    async call(call) {
      const result = await client.request({
        method: "eth_call",
        params: [call, call.block ?? "latest"]
      });
      return result;
    },
    async estimateGas(call) {
      const hex = await client.request({
        method: "eth_estimateGas",
        params: [call]
      });
      return hexToBigInt2(hex);
    },
    async getGasPrice() {
      const hex = await client.request({
        method: "eth_gasPrice",
        params: void 0
      });
      return hexToBigInt2(hex);
    },
    getTransactionReceipt,
    async getTransactionCount(address, block) {
      const hex = await client.request({
        method: "eth_getTransactionCount",
        params: [address, block ? toHex(block) : "latest"]
      });
      return Number(hexToBigInt2(hex));
    }
  };
};
var convertLog = (log2) => ({
  blockNumber: hexToBigInt2(log2.blockNumber),
  topics: [...log2.topics],
  data: log2.data,
  address: log2.address,
  ...log2.logIndex ? { logIndex: Number(hexToBigInt2(log2.logIndex)) } : {},
  ...log2.transactionHash ? { transactionHash: log2.transactionHash } : {}
});
var convertReceipt = (receipt) => ({
  blockNumber: hexToBigInt2(receipt.blockNumber),
  status: receipt.status ? hexToBigInt2(receipt.status) : BigInt(0),
  logs: receipt.logs.map(convertLog),
  gasUsed: hexToBigInt2(receipt.gasUsed)
});

// src/anon-provider.ts
var TOR_JS_DEFAULT_GATEWAYS = ["159.89.167.167:12298:uEiA-2gShJvf4zebUOQElPY_y46cFeECnyn1TqOfnIQMeTg"];
var ANON_NETWORKS = {
  nox: { specifier: "0x35133267A8F7EC35046a83DCA0f4Ca8A6e1b7A9A", specifierChainId: 11155111 },
  nym: { specifier: "0x3aCD9742C68F6d993066Ae25533e09b9e1f60eD0", specifierChainId: 1 },
  "tor-js": {
    specifier: "0x700dA3193D35fA54Cd3fBf29B66f2a2A0385659e",
    specifierChainId: 1,
    defaultConfig: { gateways: TOR_JS_DEFAULT_GATEWAYS }
  }
};
var DEFAULT_BOOTSTRAP_URLS = {
  1: "https://ethereum-rpc.publicnode.com",
  11155111: "https://sepolia.gateway.tenderly.co"
};
var ANON_WAIT_FOR_TRANSACTION = { pollIntervalMs: 4e3, timeoutMs: 3e5 };
function pickAnonNetwork(userAgent = globalThis.navigator?.userAgent ?? "") {
  if (/iPhone|iPad|iPod/.test(userAgent)) return "nym";
  const webkit = /AppleWebKit/.test(userAgent);
  const blink = /Chrome\/|Chromium\/|Edg\/|OPR\//.test(userAgent);
  return webkit && !blink ? "nym" : "nox";
}
var defaultHarnessFactory = async (init) => {
  const { AnonRpcWorker: AnonRpcWorker2 } = await Promise.resolve().then(() => (init_host(), host_exports));
  return new AnonRpcWorker2(init);
};
var AnonUnavailableError = class extends Error {
  constructor(network, reason, cause) {
    super(`anon-rpc (${network}) unavailable: ${reason}`);
    this.network = network;
    this.reason = reason;
    this.cause = cause;
    this.name = "AnonUnavailableError";
  }
  network;
  reason;
  cause;
};
function jsonRpcClient(doFetch, url, tag) {
  let id = 0;
  return {
    async request({ method, params }) {
      const res = await doFetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: ++id, method, params: params ?? [] })
      });
      if (!res.ok) throw new Error(`${tag}: endpoint returned http ${res.status}`);
      const body = await res.json();
      if (body.error) {
        throw Object.assign(new Error(body.error.message ?? `${tag}: rpc error`), {
          code: body.error.code,
          data: body.error.data
        });
      }
      return body.result;
    }
  };
}
function withTimeout(p, ms, what, onTimeout) {
  let timer;
  const t = new Promise((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${what} timed out after ${ms} ms`));
      onTimeout?.();
    }, ms);
  });
  return Promise.race([p, t]).finally(() => clearTimeout(timer));
}
var now = () => globalThis.performance?.now() ?? Date.now();
async function createAnonProvider(opts) {
  const network = !opts.network || opts.network === "auto" ? pickAnonNetwork() : opts.network;
  const info = ANON_NETWORKS[network];
  const fetchFn = opts.fetchFn ?? ((u, i) => globalThis.fetch(u, i));
  const wait = { ...ANON_WAIT_FOR_TRANSACTION, ...opts.waitForTransaction };
  const bootTimeoutMs = opts.bootTimeoutMs ?? 6e4;
  const callTimeoutMs = opts.callTimeoutMs ?? 12e4;
  const bootstrapUrl = opts.bootstrapUrls?.[info.specifierChainId] ?? DEFAULT_BOOTSTRAP_URLS[info.specifierChainId];
  const config = opts.workerConfig ?? (network === "tor-js" ? { gateways: opts.torGateways ?? TOR_JS_DEFAULT_GATEWAYS } : info.defaultConfig);
  const t0 = now();
  let harness;
  try {
    harness = await (opts.harnessFactory ?? defaultHarnessFactory)({
      address: info.specifier,
      ...config === void 0 ? {} : { config },
      preExisting: { rpcProvider: jsonRpcClient(fetchFn, bootstrapUrl, "bootstrap") }
    });
    const h2 = harness;
    await withTimeout(h2.ready, bootTimeoutMs, `anon-rpc ${network} boot`, () => h2.close());
  } catch (error) {
    harness?.close();
    const bootMs2 = Math.round(now() - t0);
    const reason = error instanceof Error ? error.message : String(error);
    if (opts.fallback === false) throw new AnonUnavailableError(network, reason, error);
    const fallbackRpcUrl = opts.fallback?.rpcUrl ?? opts.rpcUrl;
    opts.onFallback?.({ network, reason, error });
    return {
      anonymous: false,
      network,
      reason,
      error,
      fallbackRpcUrl,
      bootMs: bootMs2,
      isolationKey: opts.isolationKey,
      provider: raw(jsonRpcClient(fetchFn, fallbackRpcUrl, "direct-rpc"), { waitForTransaction: wait }),
      fetch: (u, i) => fetchFn(u, i),
      close: () => {
      }
    };
  }
  const h = harness;
  const bootMs = Math.round(now() - t0);
  const anonFetch = (url, init = {}) => {
    const signal = typeof AbortSignal.timeout === "function" ? AbortSignal.timeout(callTimeoutMs) : void 0;
    return h.fetch(url, {
      ...init,
      ...signal && !init.signal ? { signal } : {},
      ...!init.anonOptions && opts.isolationKey ? { anonOptions: { isolationKey: opts.isolationKey } } : {}
    });
  };
  return {
    anonymous: true,
    network,
    bootMs,
    isolationKey: opts.isolationKey,
    provider: raw(jsonRpcClient(anonFetch, opts.rpcUrl, `anon-rpc ${network}`), { waitForTransaction: wait }),
    fetch: anonFetch,
    close: () => h.close()
  };
}

// browser-anon/page.ts
var q = new URLSearchParams(location.search);
var net = q.get("net") ?? pickAnonNetwork();
var RPC = q.get("rpc") ?? "https://sepolia.gateway.tenderly.co";
var IP_ECHO = q.get("ip") ?? "https://api.ipify.org?format=json";
var ETH_POOL = "0x644d5A2554d36e27509254F32ccfeBe8cd58861f";
var ENTRYPOINT = "0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB";
var out = document.getElementById("out");
var log = (s) => {
  out.textContent += s + "\n";
  console.log(`[anon] ${s}`);
};
var R = { net, rpc: RPC, ua: navigator.userAgent, steps: {} };
var steps = R.steps;
async function timed(name, fn) {
  const t = performance.now();
  try {
    const v = await fn();
    const ms = Math.round(performance.now() - t);
    steps[name] = { ok: true, ms, value: typeof v === "bigint" ? v.toString() : v };
    log(`${name}: ${ms} ms -> ${JSON.stringify(steps[name] && steps[name].value)}`);
    return v;
  } catch (e) {
    const ms = Math.round(performance.now() - t);
    steps[name] = { ok: false, ms, error: e instanceof Error ? `${e.name}: ${e.message}` : String(e) };
    log(`${name}: FAILED after ${ms} ms: ${steps[name].error}`);
    return void 0;
  }
}
var ipOf = async (f) => {
  const r = await f(IP_ECHO);
  const t = await r.text();
  try {
    return JSON.parse(t).ip;
  } catch {
    return t.trim();
  }
};
async function session(key) {
  return createAnonProvider({
    rpcUrl: RPC,
    network: net,
    bootTimeoutMs: Number(q.get("boot") ?? 12e4),
    callTimeoutMs: 12e4,
    ...q.get("gw") ? { torGateways: [q.get("gw")] } : {},
    ...key ? { isolationKey: key } : {},
    onFallback: ({ reason }) => log(`FALLBACK to direct RPC (anonymous: false): ${reason}`)
  });
}
async function main() {
  log(`network ${net}, rpc ${RPC}`);
  R.directIp = await timed("direct.ipEcho", () => ipOf((u) => fetch(u)));
  await timed("direct.eth_blockNumber", () => fetch(RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_blockNumber", params: [] })
  }).then((r) => r.json()).then((j) => j.result));
  const a = await session("addr-A");
  R.anonymous = a.anonymous;
  R.bootMs = a.bootMs;
  if (!a.anonymous) R.fallbackReason = a.reason;
  log(`boot ${net}: ${a.bootMs} ms, anonymous=${a.anonymous}`);
  if (!a.anonymous) return;
  R.anonIpA1 = await timed("anon.ipEcho[keyA]#1", () => ipOf(a.fetch));
  R.anonIpA2 = await timed("anon.ipEcho[keyA]#2", () => ipOf(a.fetch));
  await timed("anon.eth_chainId", () => a.provider.getChainId());
  const bn = await timed("anon.eth_blockNumber#1", () => a.provider.getBlockNumber());
  await timed("anon.eth_blockNumber#2", () => a.provider.getBlockNumber());
  await timed("anon.eth_blockNumber#3", () => a.provider.getBlockNumber());
  await timed("anon.eth_getBalance(pool)", () => a.provider.getBalance(ETH_POOL));
  await timed("anon.eth_getTransactionCount(entrypoint)", () => a.provider.request({ method: "eth_getTransactionCount", params: [ENTRYPOINT, "latest"] }));
  R.anonIpSameWorkerKeyB = await timed("anon.ipEcho[sameWorker,keyB]", () => ipOf((u) => a.fetch(u, { anonOptions: { isolationKey: "addr-B" } })));
  R.anonIpSameWorkerKeyA3 = await timed("anon.ipEcho[keyA]#3", () => ipOf(a.fetch));
  const n = Number(q.get("ipn") ?? 0);
  if (n > 0) {
    const sample = {};
    for (const key of ["addr-A", "addr-B"]) {
      sample[key] = [];
      for (let i = 0; i < n; i++) {
        const ip = await timed(`anon.ipEcho.sample[${key}]#${i + 1}`, () => ipOf((u) => a.fetch(u, { anonOptions: { isolationKey: key } })));
        if (ip) sample[key].push(ip);
      }
    }
    R.egressSample = sample;
  }
  if (q.get("logs") !== "0" && bn) {
    for (const [i, from] of [bn - 10000n, bn - 5000n].entries()) {
      await timed(`anon.eth_getLogs(pool, 5000 blocks)#${i + 1}`, async () => (await a.provider.getLogs({ address: ETH_POOL, fromBlock: from, toBlock: from + 4999n })).length);
    }
    await timed("direct.eth_getLogs(pool, 5000 blocks)", () => fetch(RPC, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_getLogs", params: [{ address: ETH_POOL, fromBlock: "0x" + (bn - 5000n).toString(16), toBlock: "0x" + (bn - 1n).toString(16) }] })
    }).then((r) => r.json()).then((j) => j.result?.length ?? j.error));
  }
  if (q.get("kohaku") !== "0") {
    const eth = new EthClient(a.provider);
    const kb = BigInt(q.get("kblocks") ?? 15e3);
    const t = performance.now();
    const to = await timed("kohaku.getBlockNumber", () => eth.getBlockNumber());
    if (to) {
      await timed(`kohaku.getLogs(entrypoint, ${kb} blocks)`, async () => (await eth.getLogs({ address: ENTRYPOINT, fromBlock: to - kb, toBlock: to })).length);
      await timed(`kohaku.getLogs(ETH pool, ${kb} blocks)`, async () => (await eth.getLogs({ address: ETH_POOL, fromBlock: to - kb, toBlock: to })).length);
      await timed("kohaku.pool.currentRoot", async () => String(await eth.makeContractRequest(BigInt(ETH_POOL), "pool", "currentRoot")));
      await timed("kohaku.entrypoint.latestRoot", async () => String(await eth.makeContractRequest(BigInt(ENTRYPOINT), "entrypoint", "latestRoot")));
    }
    R.kohakuStepMs = Math.round(performance.now() - t);
    log(`kohaku incremental step (${kb} blocks): ${R.kohakuStepMs} ms`);
  }
  if (q.get("second") !== "0") {
    const b = await session("addr-B");
    R.secondBootMs = b.bootMs;
    R.secondAnonymous = b.anonymous;
    log(`second worker boot: ${b.bootMs} ms, anonymous=${b.anonymous}`);
    if (b.anonymous) {
      R.anonIpSecondWorker = await timed("anon.ipEcho[secondWorker]", () => ipOf(b.fetch));
      await timed("anon.eth_getBalance[secondWorker]", () => b.provider.getBalance(ETH_POOL));
    }
    b.close();
  }
  a.close();
}
main().then(() => {
  R.ok = true;
}).catch((e) => {
  R.ok = false;
  R.error = e instanceof Error ? e.stack ?? e.message : String(e);
  log(`FAILED ${R.error}`);
}).finally(() => {
  window.__anon = R;
});
/*! Bundled license information:

@noble/hashes/esm/utils.js:
@noble/hashes/esm/utils.js:
  (*! noble-hashes - MIT License (c) 2022 Paul Miller (paulmillr.com) *)
*/
