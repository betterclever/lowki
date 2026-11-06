// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";

/// @title LowkiOwnerAccount
/// @notice EIP-7702 delegate for a Lowki name-owner key: lets anyone (a Lowki operator) submit
///         and pay for a batch of calls the owner signed, executed with the owner's address as
///         `msg.sender`. With it the owner key never needs ETH, not just for registration
///         (`LowkiRegistrar`) but for everything after it: deploying its `PermissionedResolver`,
///         configuring ENSIP-30's `StealthResolver`, writing records, setting the resolver.
/// @dev Used only as a 7702 delegation target: the owner signs an EIP-7702 authorisation to
///      this contract and an EIP-712 `Execute` over the calls; the operator sends both in one
///      type-4 transaction to the owner's address. `address(this)` is then the owner's EOA, so
///      the EIP-712 domain's `verifyingContract` is the owner (OpenZeppelin's `EIP712` rebuilds
///      the separator when `address(this)` differs from the deployment), and a signature for
///      one owner cannot be replayed on another.
///
///      No owner, no upgrade, no modules: the only entrypoint is `execute`, and it runs only
///      what `address(this)` signed (or what the EOA itself sends). The nonce lives in an
///      ERC-7201 namespaced slot so it cannot collide with storage a later delegate uses.
///
///      The key this delegates is Lowki's dedicated name-owner identity key (lowki-ens
///      `identity`, `m/137'/0'/i'`), never a key that holds funds.
contract LowkiOwnerAccount is EIP712 {
    struct Call {
        address target;
        uint256 value;
        bytes data;
    }

    /// @custom:storage-location erc7201:lowki.owner-account
    struct AccountStorage {
        uint256 nonce;
    }

    bytes32 public constant CALL_TYPEHASH =
        keccak256("Call(address target,uint256 value,bytes data)");
    bytes32 public constant EXECUTE_TYPEHASH = keccak256(
        "Execute(Call[] calls,uint256 nonce,uint256 deadline)Call(address target,uint256 value,bytes data)"
    );

    /// @dev keccak256(abi.encode(uint256(keccak256("lowki.owner-account")) - 1)) & ~bytes32(uint256(0xff))
    bytes32 private constant STORAGE_SLOT =
        0xd24c05e2b0b2f2c83881d9873a29f94785f76b8e67ce98270c9577386da50d00;

    event Executed(uint256 indexed nonce, uint256 calls);

    error AuthorisationExpired(uint256 deadline);
    error InvalidSignature();
    error CallFailed(uint256 index, bytes reason);

    constructor() EIP712("Lowki Owner Account", "1") {}

    /// @notice The next nonce of this account.
    function nonce() public view returns (uint256) {
        return _storage().nonce;
    }

    /// @notice Runs `calls` in order as this account. Reverts the whole batch if one fails.
    /// @param calls The calls.
    /// @param deadline Last timestamp at which the signature is valid.
    /// @param signature This account's (the EOA's) signature over `executeDigest(calls,
    ///        nonce(), deadline)`. Ignored when the EOA calls itself.
    function execute(Call[] calldata calls, uint256 deadline, bytes calldata signature)
        external
        payable
    {
        AccountStorage storage s = _storage();
        uint256 n = s.nonce;
        if (msg.sender != address(this)) {
            if (block.timestamp > deadline) revert AuthorisationExpired(deadline);
            (address signer, ECDSA.RecoverError err,) =
                ECDSA.tryRecover(executeDigest(calls, n, deadline), signature);
            if (err != ECDSA.RecoverError.NoError || signer != address(this)) {
                revert InvalidSignature();
            }
        }
        s.nonce = n + 1;
        for (uint256 i; i < calls.length; ++i) {
            (bool ok, bytes memory ret) = calls[i].target.call{value: calls[i].value}(calls[i].data);
            if (!ok) revert CallFailed(i, ret);
        }
        emit Executed(n, calls.length);
    }

    /// @notice The EIP-712 digest of an `Execute` for this account.
    function executeDigest(Call[] calldata calls, uint256 n, uint256 deadline)
        public
        view
        returns (bytes32)
    {
        bytes32[] memory hashes = new bytes32[](calls.length);
        for (uint256 i; i < calls.length; ++i) {
            hashes[i] = keccak256(
                abi.encode(CALL_TYPEHASH, calls[i].target, calls[i].value, keccak256(calls[i].data))
            );
        }
        return _hashTypedDataV4(
            keccak256(
                abi.encode(EXECUTE_TYPEHASH, keccak256(abi.encodePacked(hashes)), n, deadline)
            )
        );
    }

    /// @notice ERC-1271, so a delegated owner can still sign `LowkiRegistrar` authorisations
    ///         (OpenZeppelin's `SignatureChecker` tries ECDSA first, then this).
    function isValidSignature(bytes32 hash, bytes calldata signature)
        external
        view
        returns (bytes4)
    {
        (address signer, ECDSA.RecoverError err,) = ECDSA.tryRecover(hash, signature);
        return err == ECDSA.RecoverError.NoError && signer == address(this)
            ? bytes4(0x1626ba7e)
            : bytes4(0xffffffff);
    }

    /// @notice ERC-1155 receipt: an ENSv2 name minted to a delegated owner with `safe` checks.
    function onERC1155Received(address, address, uint256, uint256, bytes calldata)
        external
        pure
        returns (bytes4)
    {
        return this.onERC1155Received.selector;
    }

    function onERC1155BatchReceived(
        address,
        address,
        uint256[] calldata,
        uint256[] calldata,
        bytes calldata
    ) external pure returns (bytes4) {
        return this.onERC1155BatchReceived.selector;
    }

    receive() external payable {}

    function _storage() private pure returns (AccountStorage storage s) {
        bytes32 slot = STORAGE_SLOT;
        assembly {
            s.slot := slot
        }
    }
}
