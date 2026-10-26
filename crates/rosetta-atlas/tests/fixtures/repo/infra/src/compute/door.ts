import * as gcp from "@pulumi/gcp";

export const door = new gcp.cloudrunv2.Service("door", {});
export const account = new gcp.serviceaccount.Account("door", {});
