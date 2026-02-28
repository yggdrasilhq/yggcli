import React from 'react';
import {render, Box, Text} from 'ink';

function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text color="green">ygg-cli</Text>
      <Text>Ink + Bun TUI scaffold is ready.</Text>
      <Text>Next step: implement config wizard and emit ygg.local.toml.</Text>
    </Box>
  );
}

render(<App />);
