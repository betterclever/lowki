import { encodeFunctionData, parseAbi, parseEther } from 'viem';

/** Uniswap V2 router on Sepolia, and Sepolia USDC. */
export const ROUTER = '0xeE567Fe1712Faf6149d80dA1E6934E354124CfE3';
export const USDC = '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238';
export const WETH = '0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14';

const routerAbi = parseAbi([
  'function swapExactETHForTokens(uint256 amountOutMin, address[] path, address to, uint256 deadline) payable returns (uint256[] amounts)',
]);

export function buildSwap(recipient: `0x${string}`, ethIn: string, minOut: bigint) {
  return {
    to: ROUTER,
    value: parseEther(ethIn),
    data: encodeFunctionData({
      abi: routerAbi,
      functionName: 'swapExactETHForTokens',
      args: [minOut, [WETH, USDC], recipient, BigInt(Math.floor(Date.now() / 1000) + 600)],
    }),
  } as const;
}
