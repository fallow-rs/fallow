import { Loader } from "@iso/trace/loader";
import { FetchBackend, ZipBackend } from "./sw/backends";

const loader = new Loader();
export const zip = loader.load(new ZipBackend());
export const live = loader.load(new FetchBackend());
