// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import {SignatureChecker} from "@openzeppelin/contracts/utils/cryptography/SignatureChecker.sol";

import {IPermissionedRegistry} from "@ensv2/registry/interfaces/IPermissionedRegistry.sol";
import {IRegistry} from "@ensv2/registry/interfaces/IRegistry.sol";
import {RegistryRolesLib} from "@ensv2/registry/libraries/RegistryRolesLib.sol";

/// @title LowkiRegistrar
/// @notice Gas-sponsored registrar for `<label>.<parent>.eth` (e.g. `alice.lowki.eth`) on an
///         ENSv2 `UserRegistry` (the `PermissionedRegistry` template, deployed as a
///         `VerifiableFactory` proxy of ENS's canonical implementation).
/// @dev The name's owner never needs ETH: the owner signs an EIP-712 `Register` authorisation
///      with the key that will own the name, and a Lowki operator submits it and pays the gas.
///
///      Why the owner signs at all, when the operator pays: the signature binds the owner to
///      the exact label, resolver and term, so the operator cannot attach a name to an address
///      without that address's consent, nor choose the name's resolver for it (the resolver
///      decides where funds go). `nonces[owner]` and `deadline` stop replays.
///
///      Why only operators submit: registration is free for the owner, so an open entrypoint
///      would let anyone squat every label with throwaway keys. Operators apply off-chain
///      abuse rules (sign-in, rate limits) before submitting.
///
///      Trust model. This contract holds only `ROLE_REGISTRAR` and `ROLE_RENEW` on the
///      registry's root resource. The deploy script gives no account any of ENSv2's
///      "unemancipated" root roles (set resolver/subregistry, unregister, upgrade, transfer
///      admin), so `REGISTRY.isEmancipated()` is true: neither Lowki nor this contract can
///      change, take or redirect a registered name. The owner receives `OWNER_ROLES` on the
///      name's token, including `ROLE_SET_RESOLVER`, so they point the name at a resolver they
///      control (their own `PermissionedResolver`, or ENSIP-30's `StealthResolver` with their
///      own records resolver).
contract LowkiRegistrar is EIP712, Ownable2Step {
    ////////////////////////////////////////////////////////////////////////
    // Constants and immutables
    ////////////////////////////////////////////////////////////////////////

    /// @notice EIP-712 type of the owner's authorisation.
    bytes32 public constant REGISTER_TYPEHASH = keccak256(
        "Register(string label,address owner,address resolver,uint64 duration,uint256 nonce,uint256 deadline)"
    );

    /// @notice Roles the owner receives on the name's token. The same set ENS's ETHRegistrar
    ///         grants for `.eth` names: set the resolver, attach a subregistry, delegate both,
    ///         and transfer the name.
    uint256 public constant OWNER_ROLES = RegistryRolesLib.ROLE_SET_RESOLVER
        | RegistryRolesLib.ROLE_SET_RESOLVER_ADMIN | RegistryRolesLib.ROLE_SET_SUBREGISTRY
        | RegistryRolesLib.ROLE_SET_SUBREGISTRY_ADMIN | RegistryRolesLib.ROLE_CAN_TRANSFER_ADMIN;

    /// @notice Shortest label, in bytes. Shorter labels are reserved for later allocation.
    uint256 public constant MIN_LABEL_LENGTH = 3;

    /// @notice Longest label, in bytes.
    uint256 public constant MAX_LABEL_LENGTH = 32;

    /// @notice The `UserRegistry` holding `<label>.<parent>.eth`.
    IPermissionedRegistry public immutable REGISTRY;

    /// @notice Shortest registration or renewal term, in seconds.
    uint64 public immutable MIN_DURATION;

    /// @notice Longest term: a name's expiry is never more than this far in the future.
    uint64 public immutable MAX_DURATION;

    ////////////////////////////////////////////////////////////////////////
    // Storage
    ////////////////////////////////////////////////////////////////////////

    /// @notice Accounts allowed to submit sponsored registrations and renewals.
    mapping(address account => bool) public isOperator;

    /// @notice Next authorisation nonce of each owner.
    mapping(address owner => uint256) public nonces;

    ////////////////////////////////////////////////////////////////////////
    // Events and errors
    ////////////////////////////////////////////////////////////////////////

    event OperatorSet(address indexed operator, bool enabled);
    event NameRegistered(
        uint256 indexed tokenId,
        string label,
        address indexed owner,
        address resolver,
        uint64 expiry,
        address indexed operator
    );
    event NameRenewed(uint256 indexed tokenId, string label, uint64 newExpiry);

    error NotOperator(address account);
    error InvalidLabel(string label);
    error NameNotAvailable(string label);
    error NameNotRegistered(string label);
    error InvalidOwner();
    error InvalidDuration(uint64 duration);
    error AuthorisationExpired(uint256 deadline);
    error InvalidSignature();

    ////////////////////////////////////////////////////////////////////////
    // Initialization
    ////////////////////////////////////////////////////////////////////////

    /// @param registry The `UserRegistry` for the parent name. This contract must be granted
    ///        `ROLE_REGISTRAR | ROLE_RENEW` on its root resource.
    /// @param admin Owner of this contract (manages operators).
    /// @param minDuration Shortest term, in seconds.
    /// @param maxDuration Longest term, in seconds.
    constructor(
        IPermissionedRegistry registry,
        address admin,
        uint64 minDuration,
        uint64 maxDuration
    ) EIP712("Lowki Registrar", "1") Ownable(admin) {
        if (minDuration == 0 || maxDuration < minDuration) revert InvalidDuration(maxDuration);
        REGISTRY = registry;
        MIN_DURATION = minDuration;
        MAX_DURATION = maxDuration;
    }

    modifier onlyOperator() {
        if (!isOperator[msg.sender]) revert NotOperator(msg.sender);
        _;
    }

    ////////////////////////////////////////////////////////////////////////
    // Admin
    ////////////////////////////////////////////////////////////////////////

    /// @notice Adds or removes an operator.
    function setOperator(address operator, bool enabled) external onlyOwner {
        isOperator[operator] = enabled;
        emit OperatorSet(operator, enabled);
    }

    ////////////////////////////////////////////////////////////////////////
    // Registration
    ////////////////////////////////////////////////////////////////////////

    /// @notice Registers `label` to `owner`, authorised by `owner`'s EIP-712 signature. Paid by
    ///         the calling operator.
    /// @param label The label, e.g. `alice` (see `isValidLabel`).
    /// @param owner The name's owner. An EOA signature, or ERC-1271 for a contract account.
    /// @param resolver The name's resolver, chosen by the owner (may be zero).
    /// @param duration Term in seconds, `MIN_DURATION..=MAX_DURATION`.
    /// @param deadline Last timestamp at which the authorisation is valid.
    /// @param signature The owner's signature over `registrationDigest(...)` with
    ///        `nonces[owner]`.
    /// @return tokenId The ERC-1155 token ID minted to `owner`.
    function register(
        string calldata label,
        address owner,
        address resolver,
        uint64 duration,
        uint256 deadline,
        bytes calldata signature
    ) external onlyOperator returns (uint256 tokenId) {
        if (block.timestamp > deadline) revert AuthorisationExpired(deadline);
        if (owner == address(0)) revert InvalidOwner();
        if (duration < MIN_DURATION || duration > MAX_DURATION) revert InvalidDuration(duration);
        if (!isValidLabel(label)) revert InvalidLabel(label);
        if (!available(label)) revert NameNotAvailable(label);

        bytes32 digest =
            registrationDigest(label, owner, resolver, duration, nonces[owner], deadline);
        if (!SignatureChecker.isValidSignatureNow(owner, digest, signature)) {
            revert InvalidSignature();
        }
        ++nonces[owner];

        uint64 expiry = uint64(block.timestamp) + duration;
        tokenId = REGISTRY.register(
            label, owner, IRegistry(address(0)), resolver, OWNER_ROLES, expiry
        );
        emit NameRegistered(tokenId, label, owner, resolver, expiry, msg.sender);
    }

    /// @notice Extends a registered name by `duration`, paid by the calling operator. The new
    ///         expiry may not be more than `MAX_DURATION` from now.
    function renew(string calldata label, uint64 duration) external onlyOperator {
        if (duration < MIN_DURATION) revert InvalidDuration(duration);
        uint256 labelId = uint256(keccak256(bytes(label)));
        IPermissionedRegistry.State memory state = REGISTRY.getState(labelId);
        if (state.status != IPermissionedRegistry.Status.REGISTERED) {
            revert NameNotRegistered(label);
        }
        uint64 newExpiry = state.expiry + duration;
        if (newExpiry > uint64(block.timestamp) + MAX_DURATION) revert InvalidDuration(duration);
        REGISTRY.renew(labelId, newExpiry);
        emit NameRenewed(state.tokenId, label, newExpiry);
    }

    ////////////////////////////////////////////////////////////////////////
    // Views
    ////////////////////////////////////////////////////////////////////////

    /// @notice Whether `label` can be registered now: valid and `AVAILABLE` in the registry
    ///         (never registered, expired, and not reserved).
    function available(string calldata label) public view returns (bool) {
        return isValidLabel(label)
            && REGISTRY.getStatus(uint256(keccak256(bytes(label))))
                == IPermissionedRegistry.Status.AVAILABLE;
    }

    /// @notice The EIP-712 digest the owner signs.
    function registrationDigest(
        string calldata label,
        address owner,
        address resolver,
        uint64 duration,
        uint256 nonce,
        uint256 deadline
    ) public view returns (bytes32) {
        return _hashTypedDataV4(
            keccak256(
                abi.encode(
                    REGISTER_TYPEHASH,
                    keccak256(bytes(label)),
                    owner,
                    resolver,
                    duration,
                    nonce,
                    deadline
                )
            )
        );
    }

    /// @notice Lowki label rules: 3 to 32 bytes of `a-z`, `0-9` and `-`, not starting or
    ///         ending with `-`, and without `--` in the third and fourth positions.
    /// @dev Every label that passes is already ENSIP-15 normalized (lowercase ASCII LDH, and
    ///      ENSIP-15's CheckHyphen rejects `??--`), so a name never has two spellings that
    ///      normalize differently, and wallets display it as typed. ASCII only, on purpose:
    ///      no confusables or invisible characters in payment names.
    function isValidLabel(string calldata label) public pure returns (bool) {
        bytes calldata b = bytes(label);
        uint256 n = b.length;
        if (n < MIN_LABEL_LENGTH || n > MAX_LABEL_LENGTH) return false;
        if (b[0] == "-" || b[n - 1] == "-") return false;
        if (n >= 4 && b[2] == "-" && b[3] == "-") return false;
        for (uint256 i; i < n; ++i) {
            bytes1 c = b[i];
            if (!((c >= "a" && c <= "z") || (c >= "0" && c <= "9") || c == "-")) return false;
        }
        return true;
    }

    /// @notice The EIP-712 domain separator (for clients and tests).
    function domainSeparator() external view returns (bytes32) {
        return _domainSeparatorV4();
    }
}
