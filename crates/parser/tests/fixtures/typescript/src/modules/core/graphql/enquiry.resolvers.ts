import { interestRequestService } from "../services/interest-request.service.js";
import { ListingService } from "../services/listing.service.js";

export const resolvers = {
  Query: {
    getEnquiryTeaser: async (_: unknown, { tokenAddress, chainId }: any, ctx: any) => {
      const counts = await interestRequestService.getTeaserCounts(tokenAddress, chainId);
      return counts;
    },
    getInterestRequests: async (_: unknown, { tokenAddress, chainId }: any) => {
      return interestRequestService.getRequestsForToken(tokenAddress, chainId);
    },
  },
  Mutation: {
    // Static call to contrast with the instance calls above.
    createListing: async (_: unknown, { input }: any, ctx: any) => {
      return ListingService.createListing(input.name, ctx.userId);
    },
  },
};
