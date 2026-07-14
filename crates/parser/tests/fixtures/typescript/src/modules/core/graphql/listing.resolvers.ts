import { ListingService } from "../services/listing.service.js";
import { ListingDocument } from "../models/listing.model.js";

export const resolvers = {
  Query: {
    listing: async (_: unknown, { id }: { id: string }) => {
      return ListingService.getListing(id);
    },
    listings: async () => {
      // would call ListingService.listListings() — not yet implemented
      return [];
    },
  },
  Mutation: {
    createListing: async (_: unknown, { input }: any, ctx: any) => {
      return ListingService.createListing(input.name, ctx.userId);
    },
  },
  Listing: {
    percentageRaised: async (listing: ListingDocument) => {
      // TODO: Implement calculation based on subscriptions
      // pending the order service
      return 0;
    },
    daysLeft: async (listing: ListingDocument) => {
      // TODO: Implement based on subscriptionCloseAt
      return null;
    },
  },
};
