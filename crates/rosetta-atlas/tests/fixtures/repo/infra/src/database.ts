import * as gcp from "@pulumi/gcp";

export const instance = new gcp.sql.DatabaseInstance("cells", {});
export const database = new gcp.sql.Database("records", {});
export const replica = new gcp.sql.DatabaseInstance("replica", {});
