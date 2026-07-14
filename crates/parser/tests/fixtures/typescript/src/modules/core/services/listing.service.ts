import { Listing, ListingDocument } from "../models/listing.model.js";
import "../models/order.model.js";
import { ShareClassService } from "./share-class.service.js";

// Negative cases — these must NOT produce structural edges:
// const x = "ShareClassService.fakeCall()";  // string literal
// // ShareClassService.commentedCall();       // commented out

export class ListingService {
  static async createListing(name: string, actorId: string): Promise<ListingDocument> {
    const existing = await Listing.findOne({ name });
    if (existing) throw new Error("Listing already exists");

    await ShareClassService.ensureDefaultShareClass("placeholder");

    return Listing.create({ name, code: name.toLowerCase() });
  }

  static async getListing(id: string): Promise<ListingDocument | null> {
    return Listing.findById(id);
  }

  static async updateListing(
    id: string,
    updates: Partial<ListingDocument>,
  ): Promise<ListingDocument | null> {
    return Listing.findByIdAndUpdate(id, updates, { new: true });
  }

  static async deleteListing(id: string): Promise<void> {
    await ShareClassService.deleteShareClass(id);
    await Listing.findByIdAndDelete(id);
  }
}
