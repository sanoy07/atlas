import { Listing, ListingDocument } from "../models/listing.model.js";

export class ShareClassService {
  static async ensureDefaultShareClass(listingId: string): Promise<void> {
    const listing = await Listing.findById(listingId);
    if (!listing) return;
    // ensure a default share class exists for this listing
  }

  static async deleteShareClass(shareClassId: string): Promise<void> {
    // TODO: Check if any holdings/subscriptions exist for this share class
    const listing = await Listing.findOne({ shareClasses: shareClassId });
    if (listing) {
      throw new Error("Cannot delete: active subscriptions exist");
    }
  }
}
