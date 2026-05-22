import { createRouter } from "@tanstack/react-router";
import { routeTree } from "./routeTree.gen";
import "./missing-control";

export const router = createRouter({ routeTree });
