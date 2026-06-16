import { FieldText, FieldNumber, FieldSelectWithOptions } from "./fields";
import { FieldTextarea, Box, Stack, CardA, CardB } from "./form";

// Entry point: render every component so they are all reachable. Reachability
// is required: the detector only inspects reachable React modules.
export const App = () => (
  <Box className="root" style={{}} onClick={() => {}}>
    <Stack className="col" style={{}} onClick={() => {}}>
      <FieldText label="Name" value="" error="" helpText="" name="name" onChange={() => {}} />
      <FieldNumber label="Age" value="" error="" helpText="" name="age" onChange={() => {}} />
      <FieldTextarea label="Bio" value="" error="" helpText="" name="bio" onChange={() => {}} />
      <FieldSelectWithOptions
        label="Role"
        value=""
        error=""
        helpText=""
        options={["a", "b"]}
        name="role"
        onChange={() => {}}
      />
      <CardA title="A" subtitle="s" href="#" imageUrl="x.png" />
      <CardB title="B" subtitle="s" href="#" imageUrl="y.png" />
    </Stack>
  </Box>
);
