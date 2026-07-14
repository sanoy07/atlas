import { Schema, model, Document, Types } from "mongoose";

export interface ListingDocument extends Document {
  _id: Types.ObjectId;
  name: string;
  code: string;
  targetRaiseAmount?: Types.Decimal128;
  minSubscriptionAmount?: Types.Decimal128;
}

const ListingSchema = new Schema<ListingDocument>({
  name: { type: String, required: true },
  code: { type: String, required: true, unique: true },
  targetRaiseAmount: { type: Schema.Types.Decimal128 },
  minSubscriptionAmount: { type: Schema.Types.Decimal128 },
});

export const Listing = model<ListingDocument>("Listing", ListingSchema);
