// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {IAddrResolver} from "@ens/contracts/resolvers/profiles/IAddrResolver.sol";
import {IAddressResolver} from "@ens/contracts/resolvers/profiles/IAddressResolver.sol";
import {IDataResolver} from "@ens/contracts/resolvers/profiles/IDataResolver.sol";
import {ITextResolver} from "@ens/contracts/resolvers/profiles/ITextResolver.sol";
import {Grant} from "@ensv2/access-control/interfaces/IEACGrantInitializable.sol";
import {EACBaseRolesLib} from "@ensv2/access-control/libraries/EACBaseRolesLib.sol";
import {PermissionedResolver} from "@ensv2/resolver/PermissionedResolver.sol";
import {StealthResolver} from "@fluidkey/StealthResolver.sol";
import {Scheme1Verifier} from "@fluidkey/verifiers/Scheme1Verifier.sol";
import {CCIPClient} from "@fluidkey-test/utils/CCIPClient.sol";
import {Scheme1Vectors} from "@fluidkey-test/utils/Scheme1Vectors.sol";

import {LowkiOwnerAccount} from "../src/LowkiOwnerAccount.sol";
import {LowkiFixture} from "./LowkiFixture.sol";

/// @dev The whole Lowki owner flow on namechain's local ENSv2 fixture with Fluidkey's
///      unmodified `StealthResolver` + `Scheme1Verifier`:
///      1. sponsored registration of `alice.lowki.eth` (owner signs, operator pays);
///      2. one sponsored EIP-7702 batch, signed by the owner: deploy her own
///         `PermissionedResolver` (with the legacy text record), configure `StealthResolver`
///         (records resolver, gateway URL, chain 60, meta-address = verified mode), and point
///         the name at it with her `ROLE_SET_RESOLVER`;
///      3. resolution through `UniversalResolverV2`, with the gateway played by Fluidkey's
///         `CCIPClient` answering ScopeLift-SDK vectors: honest answers resolve, tampered
///         ones revert.
///      The real gateway (lowki-server) and viem are exercised on the Sepolia fork in `e2e/`.
contract LowkiStealthFlowTest is LowkiFixture, CCIPClient {
    StealthResolver internal stealth;
    LowkiOwnerAccount internal accountImpl;
    bytes internal name;
    bytes32 internal node;
    Scheme1Vectors.Vector internal vec;

    function setUp() public {
        setUpLowki();
        stealth = new StealthResolver(rootRegistry, new Scheme1Verifier(), batchGatewayProvider);
        accountImpl = new LowkiOwnerAccount();
        name = dns("alice.lowki.eth");
        node = nodeOf("alice.lowki.eth");
        vec = Scheme1Vectors.all()[0];
        vm.setEvmVersion("prague");
    }

    function _calls(address records) internal view returns (LowkiOwnerAccount.Call[] memory c) {
        bytes[] memory init = new bytes[](1);
        init[0] = abi.encodeCall(
            PermissionedResolver.setText, (name, "stealth-meta-address[1]", "st:eth:0x...")
        );
        Grant[] memory grants = new Grant[](1);
        grants[0] = Grant(alice, EACBaseRolesLib.ALL_ROLES | EACBaseRolesLib.ADMIN_ROLES);
        string[] memory urls = new string[](1);
        urls[0] = STEALTH_GW;
        uint256[] memory chains = new uint256[](1);
        chains[0] = 60;

        c = new LowkiOwnerAccount.Call[](5);
        c[0] = LowkiOwnerAccount.Call(
            address(verifiableFactory),
            0,
            abi.encodeCall(
                verifiableFactory.deployProxy,
                (
                    address(resolverImpl),
                    uint256(node),
                    abi.encodeCall(PermissionedResolver.initialize, (grants, init))
                )
            )
        );
        c[1] = LowkiOwnerAccount.Call(
            address(stealth), 0, abi.encodeCall(StealthResolver.setConfig, (name, records, urls))
        );
        c[2] = LowkiOwnerAccount.Call(
            address(stealth), 0, abi.encodeCall(StealthResolver.setChains, (name, chains, true))
        );
        c[3] = LowkiOwnerAccount.Call(
            address(stealth),
            0,
            abi.encodeCall(
                StealthResolver.setMetaAddress, (name, abi.encodePacked(hex"01", vec.metaAddress))
            )
        );
        c[4] = LowkiOwnerAccount.Call(
            address(lowkiRegistry),
            0,
            abi.encodeCall(lowkiRegistry.setResolver, (uint256(keccak256("alice")), address(stealth)))
        );
    }

    function _sign(LowkiOwnerAccount.Call[] memory c, uint256 n, uint256 deadline)
        internal
        view
        returns (bytes memory)
    {
        bytes32 digest = LowkiOwnerAccount(payable(alice)).executeDigest(c, n, deadline);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(alicePk, digest);
        return abi.encodePacked(r, s, v);
    }

    function _setUpAlice() internal returns (address records) {
        registerAlice("alice", address(0));
        vm.signAndAttachDelegation(address(accountImpl), alicePk);
        records = verifiableFactory.predictProxyAddress(alice, uint256(node));
        LowkiOwnerAccount.Call[] memory c = _calls(records);
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig = _sign(c, 0, deadline);
        assertEq(alice.balance, 0);
        vm.prank(operator);
        LowkiOwnerAccount(payable(alice)).execute(c, deadline, sig);
        assertEq(alice.balance, 0, "owner never held ETH");
    }

    function _ur(bytes memory data) internal view returns (bool ok, bytes memory result) {
        bytes memory v;
        (ok, v) = ccipCall(
            address(universalResolver), abi.encodeCall(universalResolver.resolve, (name, data))
        );
        if (ok) {
            address resolvedBy;
            (result, resolvedBy) = abi.decode(v, (bytes, address));
            assertEq(resolvedBy, address(stealth));
        }
    }

    function test_fullFlow_verifiedResolution() public {
        address records = _setUpAlice();
        assertEq(findResolverV2(name), address(stealth));
        (address recordsResolver,) = stealth.getResolver(name);
        assertEq(recordsResolver, records);
        assertTrue(stealth.canSetMetaAddress(name, alice));
        assertFalse(stealth.canSetMetaAddress(name, operator));
        assertFalse(stealth.canSetMetaAddress(name, admin));

        // ENSIP-30 record, as clients read it for client-side derivation.
        (bool ok, bytes memory result) =
            _ur(abi.encodeCall(IDataResolver.data, (node, "stealth-meta-address[1][60]")));
        assertTrue(ok);
        assertEq(abi.decode(result, (bytes)), abi.encodePacked(hex"01", vec.metaAddress));

        // Legacy text form, forwarded to her own PermissionedResolver.
        (ok, result) =
            _ur(abi.encodeCall(ITextResolver.text, (node, "stealth-meta-address[1]")));
        assertTrue(ok);
        assertEq(abi.decode(result, (string)), "st:eth:0x...");

        // Gateway path, both address profiles.
        gwClaimed = vec.stealthAddress;
        gwVerificationData = vec.verificationData;
        (ok, result) = _ur(abi.encodeCall(IAddrResolver.addr, (node)));
        assertTrue(ok, "honest gateway, addr(node)");
        assertEq(abi.decode(result, (address)), vec.stealthAddress);
        (ok, result) = _ur(abi.encodeCall(IAddressResolver.addr, (node, 60)));
        assertTrue(ok, "honest gateway, addr(node, 60)");
        assertEq(abi.decode(result, (bytes)), abi.encodePacked(vec.stealthAddress));
    }

    function test_fullFlow_tamperedGatewayRejected() public {
        _setUpAlice();
        bytes memory q = abi.encodeCall(IAddrResolver.addr, (node));

        gwClaimed = makeAddr("attacker");
        gwVerificationData = vec.verificationData;
        (bool ok,) = _ur(q);
        assertFalse(ok, "substituted address");

        gwClaimed = vec.stealthAddress;
        bytes memory vd = vec.verificationData;
        vd[95] ^= 0x01; // S.y
        gwVerificationData = vd;
        (ok,) = _ur(q);
        assertFalse(ok, "tampered shared secret");

        vd = vec.verificationData;
        vd[0] ^= 0x01; // e
        gwVerificationData = vd;
        (ok,) = _ur(q);
        assertFalse(ok, "tampered ephemeral key");

        // Another meta-address's valid derivation is not alice's.
        Scheme1Vectors.Vector memory other = Scheme1Vectors.all()[1];
        gwClaimed = other.stealthAddress;
        gwVerificationData = other.verificationData;
        (ok,) = _ur(q);
        assertFalse(ok, "derivation for another meta-address");
    }

    function test_ownerAccount_rejectsBadSignatures() public {
        registerAlice("alice", address(0));
        vm.signAndAttachDelegation(address(accountImpl), alicePk);
        LowkiOwnerAccount acct = LowkiOwnerAccount(payable(alice));
        LowkiOwnerAccount.Call[] memory c =
            _calls(verifiableFactory.predictProxyAddress(alice, uint256(node)));
        uint256 deadline = block.timestamp + 1 hours;

        // Signed by someone else.
        bytes32 digest = acct.executeDigest(c, 0, deadline);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(uint256(keccak256("mallory")), digest);
        vm.prank(operator);
        vm.expectRevert(LowkiOwnerAccount.InvalidSignature.selector);
        acct.execute(c, deadline, abi.encodePacked(r, s, v));

        // Expired.
        bytes memory sig = _sign(c, 0, deadline);
        vm.warp(deadline + 1);
        vm.prank(operator);
        vm.expectRevert(
            abi.encodeWithSelector(LowkiOwnerAccount.AuthorisationExpired.selector, deadline)
        );
        acct.execute(c, deadline, sig);

        // Valid once, then replay fails (nonce moved).
        deadline = block.timestamp + 1 hours;
        sig = _sign(c, 0, deadline);
        vm.prank(operator);
        acct.execute(c, deadline, sig);
        assertEq(acct.nonce(), 1);
        vm.prank(operator);
        vm.expectRevert(LowkiOwnerAccount.InvalidSignature.selector);
        acct.execute(c, deadline, sig);
    }

    function test_ownerAccount_batchIsAtomic() public {
        registerAlice("alice", address(0));
        vm.signAndAttachDelegation(address(accountImpl), alicePk);
        LowkiOwnerAccount acct = LowkiOwnerAccount(payable(alice));
        LowkiOwnerAccount.Call[] memory c = new LowkiOwnerAccount.Call[](2);
        c[0] = LowkiOwnerAccount.Call(
            address(lowkiRegistry),
            0,
            abi.encodeCall(lowkiRegistry.setResolver, (uint256(keccak256("alice")), address(stealth)))
        );
        // Not alice's name: fails, so the first call is rolled back too.
        c[1] = LowkiOwnerAccount.Call(
            address(stealth), 0, abi.encodeCall(StealthResolver.clearRecords, (dns("lowki.eth")))
        );
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig = _sign(c, 0, deadline);
        vm.prank(operator);
        vm.expectRevert();
        acct.execute(c, deadline, sig);
        assertEq(lowkiRegistry.getResolver("alice"), address(0));
        assertEq(acct.nonce(), 0);
    }

    function test_ownerAccount_signaturesAreBoundToTheAccount() public {
        // Bob delegates too; alice's signed batch cannot run as bob.
        uint256 bobPk = uint256(keccak256("bob"));
        address bob = vm.addr(bobPk);
        vm.signAndAttachDelegation(address(accountImpl), alicePk);
        vm.signAndAttachDelegation(address(accountImpl), bobPk);
        LowkiOwnerAccount.Call[] memory c = new LowkiOwnerAccount.Call[](0);
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig = _sign(c, 0, deadline);
        assertTrue(
            LowkiOwnerAccount(payable(alice)).executeDigest(c, 0, deadline)
                != LowkiOwnerAccount(payable(bob)).executeDigest(c, 0, deadline)
        );
        vm.prank(operator);
        vm.expectRevert(LowkiOwnerAccount.InvalidSignature.selector);
        LowkiOwnerAccount(payable(bob)).execute(c, deadline, sig);
    }

    function test_ownerAccount_erc1271() public {
        vm.signAndAttachDelegation(address(accountImpl), alicePk);
        bytes32 h = keccak256("hello");
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(alicePk, h);
        assertEq(
            LowkiOwnerAccount(payable(alice)).isValidSignature(h, abi.encodePacked(r, s, v)),
            bytes4(0x1626ba7e)
        );
        (v, r, s) = vm.sign(uint256(keccak256("mallory")), h);
        assertEq(
            LowkiOwnerAccount(payable(alice)).isValidSignature(h, abi.encodePacked(r, s, v)),
            bytes4(0xffffffff)
        );
    }

    /// @dev `executeDigest` equals forge's own EIP-712 of the JSON typed data (array of structs),
    ///      with `verifyingContract` = the delegated EOA.
    function test_ownerAccount_digestMatchesForgeTypedData() public {
        vm.signAndAttachDelegation(address(accountImpl), alicePk);
        LowkiOwnerAccount.Call[] memory c = new LowkiOwnerAccount.Call[](2);
        c[0] = LowkiOwnerAccount.Call(address(0x1234), 5, hex"deadbeef");
        c[1] = LowkiOwnerAccount.Call(address(stealth), 0, "");
        string memory json = string.concat(
            '{"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],',
            '"Execute":[{"name":"calls","type":"Call[]"},{"name":"nonce","type":"uint256"},{"name":"deadline","type":"uint256"}],',
            '"Call":[{"name":"target","type":"address"},{"name":"value","type":"uint256"},{"name":"data","type":"bytes"}]},',
            '"primaryType":"Execute","domain":{"name":"Lowki Owner Account","version":"1","chainId":',
            vm.toString(block.chainid),
            ',"verifyingContract":"',
            vm.toString(alice),
            '"},"message":{"calls":[{"target":"0x0000000000000000000000000000000000001234","value":5,"data":"0xdeadbeef"},{"target":"',
            vm.toString(address(stealth)),
            '","value":0,"data":"0x"}],"nonce":3,"deadline":99}}'
        );
        assertEq(
            vm.eip712HashTypedData(json), LowkiOwnerAccount(payable(alice)).executeDigest(c, 3, 99)
        );
    }
}
