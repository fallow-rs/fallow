import React from "react";
import { createRoot } from "react-dom/client";
import { css } from "@emotion/react";
import styled from "@emotion/styled";
import { addons } from "storybook/manager-api";
import { IconButton } from "storybook/internal/components";
import { themes } from "storybook/theming";
import { STORY_RENDERED } from "storybook/core-events";
import { Button } from "@storybook/components";
import { ThemeProvider } from "@storybook/theming";

addons.setConfig({ theme: themes.dark });
console.log(React, createRoot, css, styled, IconButton, STORY_RENDERED, Button, ThemeProvider);
