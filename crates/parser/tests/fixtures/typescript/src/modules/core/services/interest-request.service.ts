// Singleton service — exported as an instance, not a class.
export const interestRequestService = {
  async getTeaserCounts(tokenAddress: string, chainId: number) {
    return { count: 0 };
  },

  async getRequestsForToken(tokenAddress: string, chainId: number) {
    return [];
  },
};
