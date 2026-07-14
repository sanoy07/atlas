import { Schema, model, Document, Types } from "mongoose";

export interface OrderDocument extends Document {
  _id: Types.ObjectId;
  listingId: Types.ObjectId;
  userId: Types.ObjectId;
  amount: Types.Decimal128;
}

const OrderSchema = new Schema<OrderDocument>({
  listingId: { type: Schema.Types.ObjectId, required: true },
  userId:    { type: Schema.Types.ObjectId, required: true },
  amount:    { type: Schema.Types.Decimal128, required: true },
});

export const Order = model<OrderDocument>("Order", OrderSchema);
