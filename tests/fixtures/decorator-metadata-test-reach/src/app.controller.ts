import { Body, Controller, Get, Prop } from "./decorators";
import { BarrelDto } from "./dtos";
import { AccessorDto } from "./dtos/accessor.dto";
import { CtorDto } from "./dtos/ctor.dto";
import { NullableDto } from "./dtos/nullable.dto";
import { ParamDto } from "./dtos/param.dto";
import { PlainDto } from "./dtos/plain.dto";
import { PromiseDto } from "./dtos/promise.dto";
import { PropertyDto } from "./dtos/property.dto";
import { RestDto } from "./dtos/rest.dto";
import { ReturnDto } from "./dtos/return.dto";
import type { TypeOnlyDto } from "./dtos/typeonly.dto";
import { UnionDto } from "./dtos/union.dto";

// TypeScript emits `design:paramtypes`, `design:returntype` and
// `design:type` only for decorated members, and only the outer type name.
@Controller()
export class AppController {
  @Prop() property?: PropertyDto;
  @Prop() nullable?: NullableDto | null;
  @Prop() union?: UnionDto | UnionDto;

  constructor(private readonly ctor: CtorDto) {}

  @Get()
  create(@Body() body: ParamDto, ...rest: RestDto[]): ReturnDto {
    return Object.assign(body, rest);
  }

  @Get()
  barrel(@Body() body: BarrelDto): void {
    console.log(body);
  }

  @Get()
  async list(): Promise<PromiseDto[]> {
    return [];
  }

  @Get()
  typed(body: TypeOnlyDto): void {
    console.log(body);
  }

  @Prop()
  get accessor(): AccessorDto {
    return this.ctor as never;
  }

  plain(body: PlainDto): PlainDto {
    return body;
  }

  name(): string {
    return String(this.ctor);
  }
}
