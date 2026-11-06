// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {console} from "forge-std/Script.sol";

import {IGatewayProvider} from "@ens/contracts/ccipRead/IGatewayProvider.sol";
import {NameCoder} from "@ens/contracts/utils/NameCoder.sol";
import {Grant} from "@ensv2/access-control/interfaces/IEACGrantInitializable.sol";
import {EACBaseRolesLib} from "@ensv2/access-control/libraries/EACBaseRolesLib.sol";
import {IPermissionedRegistry} from "@ensv2/registry/interfaces/IPermissionedRegistry.sol";
import {IRegistry} from "@ensv2/registry/interfaces/IRegistry.sol";
import {RegistryRolesLib} from "@ensv2/registry/libraries/RegistryRolesLib.sol";
import {UserRegistry} from "@ensv2/registry/UserRegistry.sol";
import {PermissionedResolver} from "@ensv2/resolver/PermissionedResolver.sol";
import {StealthResolver} from "@fluidkey/StealthResolver.sol";
import {Scheme1Verifier} from "@fluidkey/verifiers/Scheme1Verifier.sol";
import {IStealthVerifier} from "@fluidkey/interfaces/IStealthVerifier.sol";
import {Deploy as FluidkeyDeploy} from "@fluidkey-script/Deploy.s.sol";
import {DeployScheme1Verifier} from "@fluidkey-script/DeployScheme1Verifier.s.sol";

import {LowkiOwnerAccount} from "../src/LowkiOwnerAccount.sol";
import {LowkiRegistrar} from "../src/LowkiRegistrar.sol";
import {LowkiBase} from "./LowkiBase.sol";

/// @notice Sets up `<label>.<parent>.eth` after the parent is registered (RegisterParent):
///
///   1. Fluidkey's `Scheme1Verifier` and `StealthResolver` (unmodified, compiled from the pinned
///      vendor clone), deployed through the canonical CREATE2 deployer with Fluidkey's own salts,
///      so the addresses equal any other deployment of the same bytecode and arguments; reused
///      if already deployed.
///   2. `LowkiOwnerAccount` (EIP-7702 delegate for sponsored owner batches), CREATE2.
///   3. The subname registry: a `VerifiableFactory` proxy of ENS's canonical `UserRegistryImpl`
///      (the ENSv2 registry template), initialised with only non-emancipation-breaking root
///      roles for the Lowki admin, then asserted `isEmancipated()`.
///   4. `LowkiRegistrar` (CREATE2), granted `ROLE_REGISTRAR | ROLE_RENEW`; operator enabled.
///   5. The parent's records: Lowki's own `PermissionedResolver` proxy (url/description).
///   6. `StealthResolver.setConfig(parent, lowkiRecords, [gateway URL])`; the parent lists no
///      chains, so its own address records are static (forwarded). Registered subnames carry
///      their own config (ENSIP-30: a registered name inherits nothing), written by their owner.
///   7. On the `.eth` registry: parent subregistry = the Lowki registry, resolver =
///      `StealthResolver`.
///
///   LOWKI_GATEWAY_URL=https://<host>/v1/ens/gateway/{sender}/{data}.json \
///   forge script script/DeployLowki.s.sol --rpc-url $RPC --broadcast
///
/// Env: LOWKI_GATEWAY_URL (required), LOWKI_PARENT_LABEL (default `lowki`), LOWKI_OPERATOR
/// (default: the deployer), LOWKI_PARENT_URL (optional `url` text record), LOWKI_DEPLOYMENTS_OUT
/// (default `deployments/sepolia-lowki.json`), LOWKI_KEY_ENV.
contract DeployLowki is LowkiBase {
    uint64 public constant MIN_DURATION = 28 days;
    uint64 public constant MAX_DURATION = 366 days;
    bytes32 public constant ACCOUNT_SALT = keccak256("Lowki LowkiOwnerAccount v1");

    struct Out {
        address scheme1Verifier;
        address stealthResolver;
        address ownerAccount;
        address registry;
        address registrar;
        address parentRecords;
        address batchGatewayProvider;
        address deployer;
        address operator;
    }

    Out internal o;
    uint256 internal pk;
    string internal label;
    bytes internal parentDns;
    string internal gatewayUrl;
    uint256 internal registrySalt;
    uint256 internal recordsSalt;
    bytes32 internal registrarSalt;
    DeployScheme1Verifier internal vScript;
    FluidkeyDeploy internal rScript;

    function run() external returns (Out memory) {
        _load();
        pk = _key();
        o.deployer = vm.addr(pk);
        o.operator = vm.envOr("LOWKI_OPERATOR", o.deployer);
        label = _parentLabel();
        parentDns = _dns(string.concat(label, ".eth"));
        gatewayUrl = vm.envString("LOWKI_GATEWAY_URL");
        require(
            _ethRegistry().getState(uint256(keccak256(bytes(label)))).latestOwner == o.deployer,
            "parent is not owned by the deployer"
        );
        _predict();
        vm.startBroadcast(pk);
        _deployStealth();
        _deployRegistry();
        _deployParentRecords();
        _attach();
        vm.stopBroadcast();
        require(IPermissionedRegistry(o.registry).isEmancipated(), "Lowki registry must be emancipated");
        require(
            _factory().verifyContract(o.registry) == _ens("UserRegistryImpl"),
            "registry is not a verifiable proxy of ENS's UserRegistryImpl"
        );
        _write();
        return o;
    }

    /// @dev Pure predictions, before broadcasting, so these helper contracts are never sent.
    function _predict() internal {
        IGatewayProvider batch = _batchGatewayProvider();
        o.batchGatewayProvider = address(batch);
        vScript = new DeployScheme1Verifier();
        rScript = new FluidkeyDeploy();
        o.scheme1Verifier = vScript.predict();
        o.stealthResolver =
            rScript.predict(_rootRegistry(), IStealthVerifier(o.scheme1Verifier), batch);
        o.ownerAccount = vm.computeCreate2Address(
            ACCOUNT_SALT, keccak256(type(LowkiOwnerAccount).creationCode), CREATE2_FACTORY
        );
        registrySalt = uint256(keccak256(abi.encode("lowki-registry", label)));
        o.registry = _factory().predictProxyAddress(o.deployer, registrySalt);
        registrarSalt = keccak256(abi.encode("lowki-registrar", label));
        o.registrar = vm.computeCreate2Address(
            registrarSalt,
            keccak256(
                abi.encodePacked(
                    type(LowkiRegistrar).creationCode,
                    abi.encode(o.registry, o.deployer, MIN_DURATION, MAX_DURATION)
                )
            ),
            CREATE2_FACTORY
        );
        recordsSalt = uint256(keccak256(abi.encode("lowki-parent-records", label)));
        o.parentRecords = _factory().predictProxyAddress(o.deployer, recordsSalt);
    }

    /// @dev Fluidkey verifier + resolver (same salts and arguments as Fluidkey's scripts), and
    ///      the EIP-7702 owner-account delegate.
    function _deployStealth() internal {
        if (o.scheme1Verifier.code.length == 0) {
            require(
                address(new Scheme1Verifier{salt: vScript.SALT()}()) == o.scheme1Verifier,
                "verifier address"
            );
        }
        if (o.stealthResolver.code.length == 0) {
            require(
                address(
                    new StealthResolver{salt: rScript.SALT()}(
                        _rootRegistry(),
                        IStealthVerifier(o.scheme1Verifier),
                        IGatewayProvider(o.batchGatewayProvider)
                    )
                ) == o.stealthResolver,
                "stealth resolver address"
            );
        }
        if (o.ownerAccount.code.length == 0) {
            require(
                address(new LowkiOwnerAccount{salt: ACCOUNT_SALT}()) == o.ownerAccount,
                "owner account address"
            );
        }
    }

    /// @dev Registry (ENSv2 UserRegistry template via VerifiableFactory) and registrar.
    function _deployRegistry() internal {
        if (o.registry.code.length == 0) {
            Grant[] memory grants = new Grant[](1);
            grants[0] = Grant(o.deployer, adminRootRoles());
            _factory().deployProxy(
                _ens("UserRegistryImpl"),
                registrySalt,
                abi.encodeCall(UserRegistry.initialize, (grants))
            );
        }
        IPermissionedRegistry registry = IPermissionedRegistry(o.registry);
        if (o.registrar.code.length == 0) {
            new LowkiRegistrar{salt: registrarSalt}(registry, o.deployer, MIN_DURATION, MAX_DURATION);
        }
        uint256 registrarRoles = RegistryRolesLib.ROLE_REGISTRAR | RegistryRolesLib.ROLE_RENEW;
        if (!registry.hasRootRoles(registrarRoles, o.registrar)) {
            registry.grantRootRoles(registrarRoles, o.registrar);
        }
        (IRegistry p,) = registry.getParent();
        if (address(p) != address(_ethRegistry())) registry.setParent(_ethRegistry(), label);
        if (!LowkiRegistrar(o.registrar).isOperator(o.operator)) {
            LowkiRegistrar(o.registrar).setOperator(o.operator, true);
        }
    }

    /// @dev Lowki's own PermissionedResolver for the parent's records.
    function _deployParentRecords() internal {
        if (o.parentRecords.code.length != 0) return;
        Grant[] memory grants = new Grant[](1);
        grants[0] = Grant(o.deployer, EACBaseRolesLib.ALL_ROLES | EACBaseRolesLib.ADMIN_ROLES);
        string memory url = vm.envOr("LOWKI_PARENT_URL", string(""));
        bytes[] memory calls = new bytes[](bytes(url).length == 0 ? 1 : 2);
        calls[0] = abi.encodeCall(
            PermissionedResolver.setText,
            (parentDns, "description", "Lowki: private payments to ENSv2 stealth names")
        );
        if (calls.length == 2) {
            calls[1] = abi.encodeCall(PermissionedResolver.setText, (parentDns, "url", url));
        }
        _factory().deployProxy(
            _ens("PermissionedResolverImpl"),
            recordsSalt,
            abi.encodeCall(PermissionedResolver.initialize, (grants, calls))
        );
    }

    /// @dev StealthResolver config of the parent, then subregistry + resolver on `.eth`.
    function _attach() internal {
        string[] memory urls = new string[](1);
        urls[0] = gatewayUrl;
        StealthResolver(o.stealthResolver).setConfig(parentDns, o.parentRecords, urls);
        IPermissionedRegistry eth = _ethRegistry();
        uint256 labelId = uint256(keccak256(bytes(label)));
        if (address(eth.getSubregistry(label)) != o.registry) {
            eth.setSubregistry(labelId, IRegistry(o.registry));
        }
        if (eth.getResolver(label) != o.stealthResolver) eth.setResolver(labelId, o.stealthResolver);
    }

    /// @dev Same as the test fixture: none of these is in `UNEMANCIPATED_ROLE_BITMAP`.
    function adminRootRoles() public pure returns (uint256) {
        return RegistryRolesLib.ROLE_REGISTRAR_ADMIN | RegistryRolesLib.ROLE_RENEW_ADMIN
            | RegistryRolesLib.ROLE_REGISTER_RESERVED_ADMIN | RegistryRolesLib.ROLE_SET_PARENT
            | RegistryRolesLib.ROLE_SET_PARENT_ADMIN | RegistryRolesLib.ROLE_SET_URI
            | RegistryRolesLib.ROLE_SET_URI_ADMIN;
    }

    function _write() internal {
        string memory k = "lowki";
        vm.serializeUint(k, "chainId", block.chainid);
        vm.serializeString(k, "parentName", string.concat(label, ".eth"));
        vm.serializeString(k, "gatewayUrl", gatewayUrl);
        vm.serializeAddress(k, "Scheme1Verifier", o.scheme1Verifier);
        vm.serializeAddress(k, "StealthResolver", o.stealthResolver);
        vm.serializeAddress(k, "LowkiOwnerAccount", o.ownerAccount);
        vm.serializeAddress(k, "LowkiRegistry", o.registry);
        vm.serializeAddress(k, "LowkiRegistrar", o.registrar);
        vm.serializeAddress(k, "ParentRecords", o.parentRecords);
        vm.serializeAddress(k, "BatchGatewayProvider", o.batchGatewayProvider);
        vm.serializeAddress(k, "Operator", o.operator);
        string memory out = vm.serializeAddress(k, "Admin", o.deployer);
        string memory path =
            vm.envOr("LOWKI_DEPLOYMENTS_OUT", string("deployments/sepolia-lowki.json"));
        vm.writeJson(out, path);
        console.log("wrote", path);
        console.log("StealthResolver", o.stealthResolver);
        console.log("LowkiRegistry", o.registry);
        console.log("LowkiRegistrar", o.registrar);
    }
}
