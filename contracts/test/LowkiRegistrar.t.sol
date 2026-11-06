// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {IPermissionedRegistry} from "@ensv2/registry/interfaces/IPermissionedRegistry.sol";
import {IRegistry} from "@ensv2/registry/interfaces/IRegistry.sol";
import {RegistryRolesLib} from "@ensv2/registry/libraries/RegistryRolesLib.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";

import {LowkiOwnerAccount} from "../src/LowkiOwnerAccount.sol";
import {LowkiRegistrar} from "../src/LowkiRegistrar.sol";
import {LowkiFixture} from "./LowkiFixture.sol";

contract LowkiRegistrarTest is LowkiFixture {
    address internal someResolver = makeAddr("some-resolver");

    function setUp() public {
        setUpLowki();
    }

    ////////////////////////////////////////////////////////////////////////
    // Registry setup (as the deploy script leaves it)
    ////////////////////////////////////////////////////////////////////////

    function test_registryIsEmancipated() public view {
        assertTrue(lowkiRegistry.isEmancipated());
    }

    function test_registryIsAttachedUnderEth() public view {
        assertEq(address(ethRegistry.getSubregistry("lowki")), address(lowkiRegistry));
        (IRegistry parent, string memory label) = lowkiRegistry.getParent();
        assertEq(address(parent), address(ethRegistry));
        assertEq(label, "lowki");
    }

    function test_registryIsVerifiableProxyOfCanonicalImpl() public view {
        assertEq(verifiableFactory.verifyContract(address(lowkiRegistry)), address(userRegistryImpl));
    }

    function test_registrarHoldsOnlyRegistrarAndRenew() public view {
        uint256 root = lowkiRegistry.ROOT_RESOURCE();
        assertEq(
            lowkiRegistry.roles(root, address(registrar)),
            RegistryRolesLib.ROLE_REGISTRAR | RegistryRolesLib.ROLE_RENEW
        );
        assertEq(lowkiRegistry.roles(root, admin), adminRootRoles());
    }

    ////////////////////////////////////////////////////////////////////////
    // Registration
    ////////////////////////////////////////////////////////////////////////

    function test_register() public {
        assertTrue(registrar.available("alice"));
        uint256 tokenId = registerAlice("alice", someResolver);

        assertEq(lowkiRegistry.ownerOf(tokenId), alice);
        assertEq(lowkiRegistry.getResolver("alice"), someResolver);
        assertEq(registrar.nonces(alice), 1);
        assertFalse(registrar.available("alice"));
        IPermissionedRegistry.State memory st = lowkiRegistry.getState(uint256(keccak256("alice")));
        assertEq(uint8(st.status), uint8(IPermissionedRegistry.Status.REGISTERED));
        assertEq(st.expiry, uint64(block.timestamp + 365 days));
        assertEq(lowkiRegistry.roles(st.resource, alice), registrar.OWNER_ROLES());
        // The UniversalResolver walks root -> eth -> lowki -> alice.
        assertEq(findResolverV2(dns("alice.lowki.eth")), someResolver);
    }

    function test_register_emitsEvent() public {
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig =
            signRegistration(alicePk, "alice", alice, someResolver, 365 days, 0, deadline);
        vm.expectEmit(false, true, true, true, address(registrar));
        emit LowkiRegistrar.NameRegistered(
            0, "alice", alice, someResolver, uint64(block.timestamp + 365 days), operator
        );
        vm.prank(operator);
        registrar.register("alice", alice, someResolver, 365 days, deadline, sig);
    }

    function test_ownerControlsResolver_operatorAndAdminDoNot() public {
        uint256 tokenId = registerAlice("alice", someResolver);
        address mine = makeAddr("alice-own-resolver");

        vm.prank(alice);
        lowkiRegistry.setResolver(tokenId, mine);
        assertEq(lowkiRegistry.getResolver("alice"), mine);

        address[3] memory others = [operator, admin, address(registrar)];
        for (uint256 i; i < others.length; ++i) {
            vm.prank(others[i]);
            vm.expectRevert();
            lowkiRegistry.setResolver(tokenId, someResolver);
        }
        vm.prank(admin);
        vm.expectRevert();
        lowkiRegistry.unregister(tokenId);
    }

    function test_ownerCanTransfer() public {
        uint256 tokenId = registerAlice("alice", someResolver);
        address bob = makeAddr("bob");
        vm.prank(alice);
        lowkiRegistry.safeTransferFrom(alice, bob, tokenId, 1, "");
        uint256 newId = lowkiRegistry.getTokenId(uint256(keccak256("alice")));
        assertEq(lowkiRegistry.ownerOf(newId), bob);
    }

    function test_register_onlyOperator() public {
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig =
            signRegistration(alicePk, "alice", alice, someResolver, 365 days, 0, deadline);
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(LowkiRegistrar.NotOperator.selector, alice));
        registrar.register("alice", alice, someResolver, 365 days, deadline, sig);
    }

    function test_register_rejectsWrongSigner() public {
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig = signRegistration(
            uint256(keccak256("mallory")), "alice", alice, someResolver, 365 days, 0, deadline
        );
        vm.prank(operator);
        vm.expectRevert(LowkiRegistrar.InvalidSignature.selector);
        registrar.register("alice", alice, someResolver, 365 days, deadline, sig);
    }

    function test_register_bindsEveryField() public {
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig =
            signRegistration(alicePk, "alice", alice, someResolver, 365 days, 0, deadline);
        vm.startPrank(operator);
        vm.expectRevert(LowkiRegistrar.InvalidSignature.selector);
        registrar.register("alicf", alice, someResolver, 365 days, deadline, sig);
        vm.expectRevert(LowkiRegistrar.InvalidSignature.selector);
        registrar.register("alice", alice, makeAddr("operator-choice"), 365 days, deadline, sig);
        vm.expectRevert(LowkiRegistrar.InvalidSignature.selector);
        registrar.register("alice", alice, someResolver, 300 days, deadline, sig);
        vm.expectRevert(LowkiRegistrar.InvalidSignature.selector);
        registrar.register("alice", alice, someResolver, 365 days, deadline + 1, sig);
        vm.stopPrank();
    }

    function test_register_noReplay() public {
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig =
            signRegistration(alicePk, "alice", alice, someResolver, 365 days, 0, deadline);
        vm.prank(operator);
        registrar.register("alice", alice, someResolver, 365 days, deadline, sig);
        // Same signature for another label is invalid (label bound), and the nonce moved on.
        vm.prank(operator);
        vm.expectRevert(LowkiRegistrar.InvalidSignature.selector);
        registrar.register("alice2", alice, someResolver, 365 days, deadline, sig);
    }

    function test_register_deadline() public {
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig =
            signRegistration(alicePk, "alice", alice, someResolver, 365 days, 0, deadline);
        vm.warp(deadline + 1);
        vm.prank(operator);
        vm.expectRevert(
            abi.encodeWithSelector(LowkiRegistrar.AuthorisationExpired.selector, deadline)
        );
        registrar.register("alice", alice, someResolver, 365 days, deadline, sig);
    }

    function test_register_taken() public {
        registerAlice("alice", someResolver);
        uint256 bobPk = uint256(keccak256("bob"));
        address bob = vm.addr(bobPk);
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig = signRegistration(bobPk, "alice", bob, someResolver, 365 days, 0, deadline);
        vm.prank(operator);
        vm.expectRevert(abi.encodeWithSelector(LowkiRegistrar.NameNotAvailable.selector, "alice"));
        registrar.register("alice", bob, someResolver, 365 days, deadline, sig);
    }

    function test_register_afterExpiryGoesToNewOwner() public {
        registerAlice("alice", someResolver);
        vm.warp(block.timestamp + 365 days + 1);
        assertTrue(registrar.available("alice"));
        uint256 bobPk = uint256(keccak256("bob"));
        address bob = vm.addr(bobPk);
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig = signRegistration(bobPk, "alice", bob, address(0), 365 days, 0, deadline);
        vm.prank(operator);
        uint256 tokenId = registrar.register("alice", bob, address(0), 365 days, deadline, sig);
        assertEq(lowkiRegistry.ownerOf(tokenId), bob);
        // Alice's old roles do not survive: the resource was renewed.
        IPermissionedRegistry.State memory st = lowkiRegistry.getState(uint256(keccak256("alice")));
        assertEq(lowkiRegistry.roles(st.resource, alice), 0);
    }

    function test_register_reservedLabelUnavailable() public {
        // The admin can hold labels back by reserving them (status RESERVED, no token).
        vm.startPrank(admin);
        lowkiRegistry.grantRootRoles(RegistryRolesLib.ROLE_REGISTRAR, admin);
        lowkiRegistry.register(
            "support", address(0), noRegistry(), address(0), 0, uint64(block.timestamp + 3650 days)
        );
        vm.stopPrank();
        assertFalse(registrar.available("support"));
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig =
            signRegistration(alicePk, "support", alice, someResolver, 365 days, 0, deadline);
        vm.prank(operator);
        vm.expectRevert(
            abi.encodeWithSelector(LowkiRegistrar.NameNotAvailable.selector, "support")
        );
        registrar.register("support", alice, someResolver, 365 days, deadline, sig);
    }

    function test_register_duration() public {
        uint256 deadline = block.timestamp + 1 hours;
        uint64[2] memory bad = [MIN_DURATION - 1, MAX_DURATION + 1];
        for (uint256 i; i < 2; ++i) {
            bytes memory sig =
                signRegistration(alicePk, "alice", alice, someResolver, bad[i], 0, deadline);
            vm.prank(operator);
            vm.expectRevert(
                abi.encodeWithSelector(LowkiRegistrar.InvalidDuration.selector, bad[i])
            );
            registrar.register("alice", alice, someResolver, bad[i], deadline, sig);
        }
    }

    function test_register_zeroOwner() public {
        vm.prank(operator);
        vm.expectRevert(LowkiRegistrar.InvalidOwner.selector);
        registrar.register("alice", address(0), someResolver, 365 days, block.timestamp, "");
    }

    function test_register_invalidLabel() public {
        string[8] memory bad =
            ["ab", "Alice", "-alice", "alice-", "xn--abc", "al ice", unicode"álice", "a_b"];
        for (uint256 i; i < bad.length; ++i) {
            uint256 deadline = block.timestamp + 1 hours;
            bytes memory sig =
                signRegistration(alicePk, bad[i], alice, someResolver, 365 days, 0, deadline);
            vm.prank(operator);
            vm.expectRevert(abi.encodeWithSelector(LowkiRegistrar.InvalidLabel.selector, bad[i]));
            registrar.register(bad[i], alice, someResolver, 365 days, deadline, sig);
        }
    }

    ////////////////////////////////////////////////////////////////////////
    // Label rules
    ////////////////////////////////////////////////////////////////////////

    function test_isValidLabel_cases() public view {
        assertTrue(registrar.isValidLabel("abc"));
        assertTrue(registrar.isValidLabel("a-b"));
        assertTrue(registrar.isValidLabel("a1-2b"));
        assertTrue(registrar.isValidLabel("ab-c-d"));
        assertTrue(registrar.isValidLabel("0x00"));
        assertTrue(registrar.isValidLabel("a--b")); // positions 1-2, allowed by ENSIP-15 too
        assertTrue(registrar.isValidLabel("abcdefghijklmnopqrstuvwxyz012345")); // 32
        assertFalse(registrar.isValidLabel(""));
        assertFalse(registrar.isValidLabel("ab"));
        assertFalse(registrar.isValidLabel("abcdefghijklmnopqrstuvwxyz0123456")); // 33
        assertFalse(registrar.isValidLabel("ab--"));
        assertFalse(registrar.isValidLabel("ab--c"));
        assertFalse(registrar.isValidLabel("xn--80ak6aa92e"));
        assertFalse(registrar.isValidLabel("-ab"));
        assertFalse(registrar.isValidLabel("ab-"));
        assertFalse(registrar.isValidLabel("a.b"));
        assertFalse(registrar.isValidLabel("ABC"));
        assertFalse(registrar.isValidLabel(unicode"abc‍"));
    }

    /// @dev Differential against a straightforward reference.
    function testFuzz_isValidLabel(bytes memory raw) public view {
        vm.assume(raw.length < 40);
        // Bias toward the alphabet so valid labels actually occur.
        bytes memory alphabet = "abcdefghijklmnopqrstuvwxyz0123456789-A_.";
        for (uint256 i; i < raw.length; ++i) {
            raw[i] = alphabet[uint8(raw[i]) % alphabet.length];
        }
        assertEq(registrar.isValidLabel(string(raw)), _reference(raw));
    }

    function _reference(bytes memory b) internal pure returns (bool) {
        if (b.length < 3 || b.length > 32) return false;
        for (uint256 i; i < b.length; ++i) {
            bool ok = (b[i] >= "a" && b[i] <= "z") || (b[i] >= "0" && b[i] <= "9") || b[i] == "-";
            if (!ok) return false;
        }
        if (b[0] == "-" || b[b.length - 1] == "-") return false;
        if (b.length >= 4 && b[2] == "-" && b[3] == "-") return false;
        return true;
    }

    ////////////////////////////////////////////////////////////////////////
    // EIP-712
    ////////////////////////////////////////////////////////////////////////

    function test_digestMatchesEip712() public view {
        bytes32 domain = keccak256(
            abi.encode(
                keccak256(
                    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                ),
                keccak256("Lowki Registrar"),
                keccak256("1"),
                block.chainid,
                address(registrar)
            )
        );
        assertEq(registrar.domainSeparator(), domain);
        bytes32 structHash = keccak256(
            abi.encode(
                keccak256(
                    "Register(string label,address owner,address resolver,uint64 duration,uint256 nonce,uint256 deadline)"
                ),
                keccak256("alice"),
                alice,
                someResolver,
                uint64(365 days),
                uint256(7),
                uint256(1234)
            )
        );
        assertEq(
            this.digestOf("alice", alice, someResolver, 365 days, 7, 1234),
            keccak256(abi.encodePacked(hex"1901", domain, structHash))
        );
    }

    /// @dev The same digest through forge's own EIP-712 implementation (JSON typed data).
    function test_digestMatchesForgeTypedData() public view {
        string memory json = string.concat(
            '{"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],',
            '"Register":[{"name":"label","type":"string"},{"name":"owner","type":"address"},{"name":"resolver","type":"address"},{"name":"duration","type":"uint64"},{"name":"nonce","type":"uint256"},{"name":"deadline","type":"uint256"}]},',
            '"primaryType":"Register","domain":{"name":"Lowki Registrar","version":"1","chainId":',
            vm.toString(block.chainid),
            ',"verifyingContract":"',
            vm.toString(address(registrar)),
            '"},"message":{"label":"alice","owner":"',
            vm.toString(alice),
            '","resolver":"',
            vm.toString(someResolver),
            '","duration":31536000,"nonce":7,"deadline":1234}}'
        );
        assertEq(
            vm.eip712HashTypedData(json),
            this.digestOf("alice", alice, someResolver, 365 days, 7, 1234)
        );
    }

    ////////////////////////////////////////////////////////////////////////
    // Owner as an EIP-7702 account (ERC-1271 path)
    ////////////////////////////////////////////////////////////////////////

    function test_register_delegatedOwner() public {
        vm.setEvmVersion("prague"); // EIP-7702; the contracts themselves target cancun
        LowkiOwnerAccount impl = new LowkiOwnerAccount();
        vm.signAndAttachDelegation(address(impl), alicePk);
        assertGt(alice.code.length, 0);
        uint256 tokenId = registerAlice("alice", someResolver);
        assertEq(lowkiRegistry.ownerOf(tokenId), alice);
    }

    ////////////////////////////////////////////////////////////////////////
    // Renewal and admin
    ////////////////////////////////////////////////////////////////////////

    function test_renew() public {
        registerAlice("alice", someResolver);
        uint64 before = lowkiRegistry.getExpiry(uint256(keccak256("alice")));
        vm.warp(block.timestamp + 200 days);
        vm.prank(operator);
        registrar.renew("alice", 100 days);
        assertEq(lowkiRegistry.getExpiry(uint256(keccak256("alice"))), before + 100 days);
    }

    function test_renew_capped() public {
        registerAlice("alice", someResolver);
        vm.prank(operator);
        vm.expectRevert(abi.encodeWithSelector(LowkiRegistrar.InvalidDuration.selector, 30 days));
        registrar.renew("alice", 30 days); // would end 395 days from now
    }

    function test_renew_onlyOperatorAndRegistered() public {
        vm.prank(operator);
        vm.expectRevert(abi.encodeWithSelector(LowkiRegistrar.NameNotRegistered.selector, "nobody"));
        registrar.renew("nobody", 30 days);
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(LowkiRegistrar.NotOperator.selector, alice));
        registrar.renew("alice", 30 days);
    }

    function test_setOperator_onlyOwner() public {
        vm.prank(operator);
        vm.expectRevert(
            abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, operator)
        );
        registrar.setOperator(operator, false);
        vm.prank(admin);
        registrar.setOperator(operator, false);
        assertFalse(registrar.isOperator(operator));
    }

    function test_registrarCannotTouchRegistryBeyondItsRoles() public {
        uint256 tokenId = registerAlice("alice", someResolver);
        vm.startPrank(address(registrar));
        vm.expectRevert();
        lowkiRegistry.setResolver(tokenId, address(1));
        vm.expectRevert();
        lowkiRegistry.grantRootRoles(RegistryRolesLib.ROLE_SET_RESOLVER, address(registrar));
        vm.stopPrank();
    }

    function test_constructor_rejectsBadDurations() public {
        vm.expectRevert(abi.encodeWithSelector(LowkiRegistrar.InvalidDuration.selector, 1));
        new LowkiRegistrar(lowkiRegistry, admin, 2, 1);
    }
}
