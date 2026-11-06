// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {IRegistryURIRenderer} from "@ensv2/registry/interfaces/IRegistryURIRenderer.sol";
import {IRegistry} from "@ensv2/registry/interfaces/IRegistry.sol";
import {RegistryRolesLib} from "@ensv2/registry/libraries/RegistryRolesLib.sol";
import {UserRegistry} from "@ensv2/registry/UserRegistry.sol";
import {PermissionedResolver} from "@ensv2/resolver/PermissionedResolver.sol";
import {Grant} from "@ensv2/access-control/interfaces/IEACGrantInitializable.sol";
import {NameCoder} from "@ens/contracts/utils/NameCoder.sol";
import {V2Fixture} from "~test/fixtures/V2Fixture.sol";

import {LowkiRegistrar} from "../src/LowkiRegistrar.sol";

/// @dev namechain's own ENSv2 fixture (root + `.eth` registries, UniversalResolverV2) plus
///      `lowki.eth` set up exactly as `script/DeployLowki.s.sol` does on Sepolia: a
///      `UserRegistry` proxy from `VerifiableFactory`, emancipated (no unemancipated root role
///      held by anyone), with `LowkiRegistrar` holding `ROLE_REGISTRAR | ROLE_RENEW`.
abstract contract LowkiFixture is V2Fixture {
    uint64 internal constant MIN_DURATION = 28 days;
    uint64 internal constant MAX_DURATION = 366 days;

    UserRegistry internal userRegistryImpl;
    PermissionedResolver internal resolverImpl;
    UserRegistry internal lowkiRegistry;
    LowkiRegistrar internal registrar;

    address internal admin = makeAddr("lowki-admin");
    address internal operator = makeAddr("lowki-operator");
    uint256 internal alicePk = uint256(keccak256("alice owner key"));
    address internal alice;

    function setUpLowki() internal {
        deployV2Fixture();
        alice = vm.addr(alicePk);
        userRegistryImpl =
            new UserRegistry(labelStore, IRegistryURIRenderer(address(0)), address(this));
        resolverImpl = new PermissionedResolver(address(this));

        Grant[] memory grants = new Grant[](1);
        grants[0] = Grant(admin, adminRootRoles());
        lowkiRegistry = UserRegistry(
            verifiableFactory.deployProxy(
                address(userRegistryImpl),
                uint256(keccak256("lowki")),
                abi.encodeCall(UserRegistry.initialize, (grants))
            )
        );
        ethRegistry.register(
            "lowki",
            admin,
            lowkiRegistry,
            address(0),
            _ethTokenRoles(),
            uint64(block.timestamp + 365 days)
        );
        registrar = new LowkiRegistrar(lowkiRegistry, admin, MIN_DURATION, MAX_DURATION);
        vm.startPrank(admin);
        lowkiRegistry.grantRootRoles(
            RegistryRolesLib.ROLE_REGISTRAR | RegistryRolesLib.ROLE_RENEW, address(registrar)
        );
        lowkiRegistry.setParent(ethRegistry, "lowki");
        registrar.setOperator(operator, true);
        vm.stopPrank();
    }

    /// @dev What the Lowki admin keeps on the registry's root resource: the admin halves of
    ///      the registrar roles (to install or replace a registrar), reservations, the parent
    ///      pointer and metadata URI. None of these is in `UNEMANCIPATED_ROLE_BITMAP`.
    function adminRootRoles() internal pure returns (uint256) {
        return RegistryRolesLib.ROLE_REGISTRAR_ADMIN | RegistryRolesLib.ROLE_RENEW_ADMIN
            | RegistryRolesLib.ROLE_REGISTER_RESERVED_ADMIN | RegistryRolesLib.ROLE_SET_PARENT
            | RegistryRolesLib.ROLE_SET_PARENT_ADMIN | RegistryRolesLib.ROLE_SET_URI
            | RegistryRolesLib.ROLE_SET_URI_ADMIN;
    }

    function signRegistration(
        uint256 pk,
        string memory label,
        address owner,
        address resolver,
        uint64 duration,
        uint256 nonce,
        uint256 deadline
    ) internal view returns (bytes memory) {
        bytes32 digest = this.digestOf(label, owner, resolver, duration, nonce, deadline);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);
        return abi.encodePacked(r, s, v);
    }

    /// @dev External so `string memory` becomes calldata for the registrar's view.
    function digestOf(
        string calldata label,
        address owner,
        address resolver,
        uint64 duration,
        uint256 nonce,
        uint256 deadline
    ) external view returns (bytes32) {
        return registrar.registrationDigest(label, owner, resolver, duration, nonce, deadline);
    }

    function registerAlice(string memory label, address resolver) internal returns (uint256) {
        uint256 deadline = block.timestamp + 1 hours;
        bytes memory sig = signRegistration(
            alicePk, label, alice, resolver, 365 days, registrar.nonces(alice), deadline
        );
        vm.prank(operator);
        return registrar.register(label, alice, resolver, 365 days, deadline, sig);
    }

    function dns(string memory name) internal pure returns (bytes memory) {
        return NameCoder.encode(name);
    }

    function nodeOf(string memory name) internal pure returns (bytes32) {
        return NameCoder.namehash(NameCoder.encode(name), 0);
    }

    function noRegistry() internal pure returns (IRegistry) {
        return IRegistry(address(0));
    }
}
