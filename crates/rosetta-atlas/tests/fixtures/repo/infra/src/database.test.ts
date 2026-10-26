import * as gcp from "@pulumi/gcp";

export const scratch = new gcp.storage.Bucket("scratch", {});
