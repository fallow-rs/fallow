import { Capacitor } from "@capacitor/core";
import { NestFactory } from "@nestjs/core";

export const platform = () => Capacitor.getPlatform();
export const create = (module) => NestFactory.create(module);
