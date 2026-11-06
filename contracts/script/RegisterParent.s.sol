// SPDX-License-Identifier: MIT
pragma solidity 0.8.25;

import {console} from "forge-std/Script.sol";

import {IRegistry} from "@ensv2/registry/interfaces/IRegistry.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

import {LowkiBase} from "./LowkiBase.sol";

interface IMockUSDC is IERC20 {
    function mint(address to, uint256 amount) external;
}

/// @notice Registers the parent `.eth` name (default `lowki.eth`) through ENSv2's ETHRegistrar:
///         commit, wait `MIN_COMMITMENT_AGE` (60 s on Sepolia), register, paying in MockUSDC
///         (which has a public `mint`, minted here if the balance is short).
///
///   # 1. availability report (no key needed)
///   forge script script/RegisterParent.s.sol --sig "check()" --rpc-url $RPC
///   # 2. commit (LOWKI_COMMIT_SECRET: 32 random bytes, keep it until step 3)
///   forge script script/RegisterParent.s.sol --sig "commit()" --rpc-url $RPC --broadcast
///   # 3. after >= 60 s
///   forge script script/RegisterParent.s.sol --sig "register()" --rpc-url $RPC --broadcast
///
/// Env: LOWKI_PARENT_LABEL (default `lowki`), LOWKI_PARENT_SUFFIX (used only if the label is
/// taken: registers `<label>-<suffix>` instead), LOWKI_COMMIT_SECRET, LOWKI_PARENT_DURATION
/// (seconds, default 1 year), LOWKI_KEY_ENV (name of the env var holding the key, default
/// LOWKI_SEPOLIA_KEY).
contract RegisterParent is LowkiBase {
    function check() external {
        _load();
        string memory base = _parentLabel();
        _logState(base);
        string memory suffix = vm.envOr("LOWKI_PARENT_SUFFIX", string(""));
        if (bytes(suffix).length != 0) _logState(string.concat(base, "-", suffix));
        string memory label = _choose();
        (uint256 b, uint256 p) = _ethRegistrar().getRegisterPrice(
            label, _duration(), IERC20(_ens("MockUSDC"))
        );
        console.log(string.concat("would register ", label, ".eth; price (MockUSDC, 6 dp) base"), b);
        console.log("premium", p);
    }

    function commit() external {
        _load();
        string memory label = _choose();
        uint256 pk = _key();
        address owner = vm.addr(pk);
        if (_ownedByUs(label)) {
            console.log(string.concat(label, ".eth is already registered to us; nothing to commit"));
            return;
        }
        bytes32 commitment = _ethRegistrar().makeCommitment(
            label, owner, _secret(), IRegistry(address(0)), address(0), _duration(), bytes32(0)
        );
        vm.startBroadcast(pk);
        _ethRegistrar().commit(commitment);
        vm.stopBroadcast();
        console.log(string.concat("committed ", label, ".eth for"), owner);
        console.log("wait MIN_COMMITMENT_AGE seconds, then run register()");
    }

    function register() external returns (string memory label) {
        _load();
        label = _choose();
        uint256 pk = _key();
        address owner = vm.addr(pk);
        if (_ownedByUs(label)) {
            console.log(string.concat(label, ".eth is already registered to us"));
            return label;
        }
        IMockUSDC usdc = IMockUSDC(_ens("MockUSDC"));
        (uint256 base, uint256 premium) =
            _ethRegistrar().getRegisterPrice(label, _duration(), usdc);
        uint256 total = base + premium;

        vm.startBroadcast(pk);
        if (usdc.balanceOf(owner) < total) usdc.mint(owner, total - usdc.balanceOf(owner));
        usdc.approve(address(_ethRegistrar()), total);
        _ethRegistrar().register(
            label,
            owner,
            _secret(),
            IRegistry(address(0)),
            address(0),
            _duration(),
            usdc,
            bytes32(0)
        );
        vm.stopBroadcast();
        _logState(label);
        require(_status(label).latestOwner == owner, "registration did not land");
    }

    /// @dev `LOWKI_PARENT_LABEL` if available (or already ours), else `<label>-<suffix>`.
    function _choose() internal view returns (string memory) {
        string memory base = _parentLabel();
        if (_ethRegistrar().isAvailable(base) || _ownedByUs(base)) return base;
        string memory suffix = vm.envOr("LOWKI_PARENT_SUFFIX", string(""));
        require(
            bytes(suffix).length != 0,
            string.concat(base, ".eth is taken: set LOWKI_PARENT_SUFFIX to use <label>-<suffix>.eth")
        );
        string memory alt = string.concat(base, "-", suffix);
        require(
            _ethRegistrar().isAvailable(alt) || _ownedByUs(alt),
            string.concat(alt, ".eth is taken too")
        );
        console.log(string.concat(base, ".eth is taken; using ", alt, ".eth"));
        return alt;
    }

    function _ownedByUs(string memory label) internal view returns (bool) {
        string memory envName = vm.envOr("LOWKI_KEY_ENV", string("LOWKI_SEPOLIA_KEY"));
        if (!vm.envExists(envName)) return false;
        return _status(label).latestOwner == vm.addr(vm.envUint(envName))
            && uint256(_status(label).status) == 2;
    }

    function _secret() internal view returns (bytes32) {
        return vm.envBytes32("LOWKI_COMMIT_SECRET");
    }

    function _duration() internal view returns (uint64) {
        return uint64(vm.envOr("LOWKI_PARENT_DURATION", uint256(365 days)));
    }
}
