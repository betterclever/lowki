// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {Script, console} from "forge-std/Script.sol";

import {IGatewayProvider} from "@ens/contracts/ccipRead/IGatewayProvider.sol";
import {NameCoder} from "@ens/contracts/utils/NameCoder.sol";
import {IETHRegistrar} from "@ensv2/registrar/interfaces/IETHRegistrar.sol";
import {IPermissionedRegistry} from "@ensv2/registry/interfaces/IPermissionedRegistry.sol";
import {IRegistry} from "@ensv2/registry/interfaces/IRegistry.sol";
import {VerifiableFactory} from "@ensdomains/verifiable-factory/VerifiableFactory.sol";

/// @dev Shared by the Lowki scripts. Every ENS address comes from one JSON file
///      (`deployments/sepolia-ensv2.json` by default, `LOWKI_ENS_CONFIG` to override), never
///      from constants in code. The broadcasting key is read from the environment variable
///      named by `LOWKI_KEY_ENV` (default `LOWKI_SEPOLIA_KEY`) and never printed.
abstract contract LowkiBase is Script {
    string internal json;

    function _load() internal {
        string memory path = vm.envOr("LOWKI_ENS_CONFIG", string("deployments/sepolia-ensv2.json"));
        json = vm.readFile(path);
        uint256 chainId = vm.parseJsonUint(json, ".chainId");
        require(block.chainid == chainId, "chain id does not match the ENS config file");
    }

    function _ens(string memory key) internal view returns (address) {
        return vm.parseJsonAddress(json, string.concat(".ens.", key));
    }

    function _key() internal view returns (uint256) {
        string memory envName = vm.envOr("LOWKI_KEY_ENV", string("LOWKI_SEPOLIA_KEY"));
        return vm.envUint(envName);
    }

    function _parentLabel() internal view returns (string memory) {
        return vm.envOr("LOWKI_PARENT_LABEL", string("lowki"));
    }

    function _ethRegistry() internal view returns (IPermissionedRegistry) {
        return IPermissionedRegistry(_ens("ETHRegistry"));
    }

    function _ethRegistrar() internal view returns (IETHRegistrar) {
        return IETHRegistrar(_ens("ETHRegistrar"));
    }

    function _factory() internal view returns (VerifiableFactory) {
        return VerifiableFactory(_ens("VerifiableFactory"));
    }

    function _rootRegistry() internal view returns (IRegistry) {
        return IRegistry(_ens("RootRegistry"));
    }

    /// @dev Read on-chain: the deployments page does not list it.
    function _batchGatewayProvider() internal view returns (IGatewayProvider) {
        (bool ok, bytes memory v) =
            _ens("UniversalResolverV2").staticcall(abi.encodeWithSignature("BATCH_GATEWAY_PROVIDER()"));
        require(ok && v.length == 32, "cannot read BATCH_GATEWAY_PROVIDER");
        return abi.decode(v, (IGatewayProvider));
    }

    function _dns(string memory name) internal pure returns (bytes memory) {
        return NameCoder.encode(name);
    }

    function _status(string memory label) internal view returns (IPermissionedRegistry.State memory) {
        return _ethRegistry().getState(uint256(keccak256(bytes(label))));
    }

    function _logState(string memory label) internal view {
        IPermissionedRegistry.State memory s = _status(label);
        bool available = _ethRegistrar().isAvailable(label);
        console.log(
            string.concat(
                label,
                ".eth: status=",
                vm.toString(uint256(s.status)),
                " (0 available, 1 reserved, 2 registered) expiry=",
                vm.toString(uint256(s.expiry)),
                " owner=",
                vm.toString(s.latestOwner),
                " registrar.isAvailable=",
                available ? "true" : "false"
            )
        );
    }
}
