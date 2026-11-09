import { Service } from './service';

export const main = async (): Promise<number> => {
  const awaited = await Service.create();
  const built = Service.build();
  return awaited.viaAwait().length + built.viaSync().length;
};
