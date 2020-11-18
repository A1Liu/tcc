/* eslint-disable no-console */
/* eslint-disable react/prop-types */
import React, { createContext, useEffect, useState, useRef } from "react";
import Highlight, { defaultProps } from "prism-react-renderer";
import theme from "prism-react-renderer/themes/vsDark";

const FileUploadContext = createContext({
  files: {}, // array of files
  currentFile: "",
  styles: {},
  highlight: (currCode, language) => {
    console.log(currCode, language);
  },
  setCurrentFile: (file) => console.log(file),
  addFile: (file) => console.log(file),
  addListener: (messages, listener) => console.log(messages, listener),
  sockSend: (command, data) => console.log(command, data),
  addGlobalListener: (listener) => console.log(listener),
});

const starter = `// Online C compiler to run C program online
#include <stdio.h>

int main() {
    // Write C code here
    printf("Hello world");

    return 0;
}
`;

export const FileUploadProvider = ({ children }) => {
  const [files, setFiles] = useState({ "main.c": starter });
  const [currentFile, setCurrentFile] = useState("main.c");
  const open = useRef(false);
  const backlog = useRef([]);
  const globalListeners = useRef([]);
  const listeners = useRef({});
  const socket = useRef(undefined);

  const highlight = (currCode, language) => (
    <Highlight
      {...defaultProps}
      theme={theme}
      code={currCode}
      language={language}
    >
      {({ tokens, getLineProps, getTokenProps }) => (
        <>
          {tokens.map((line, i) => (
            <div {...getLineProps({ line, key: i })}>
              {line.map((token, key) => (
                <span {...getTokenProps({ token, key })} />
              ))}
            </div>
          ))}
        </>
      )}
    </Highlight>
  );

  const styles = {
    root: {
      boxSizing: "border-box",
      fontFamily: '"Dank Mono", "Fira Code", monospace',
      ...theme.plain,
      outline: 0,
      overflow: "scroll",
    },
  };

  const sockSend = (command, data) => {
    const value = JSON.stringify({ command, data });
    if (!open.current) return backlog.current.push(value);

    if (backlog.current.length !== 0) {
      backlog.current.forEach((item) => socket.current.send(item));
      backlog.current = [];
    }

    return socket.current.send(value);
  };

  if (socket.current === undefined) {
    const sock = new WebSocket("wss://tci.a1liu.com");

    sock.onopen = (_evt) => {
      console.log("now open for business");

      backlog.current.forEach((item) => sock.send(item));
      backlog.current = [];
      open.current = true;
    };

    sock.onmessage = (evt) => {
      console.log(evt.data);
      const resp = JSON.parse(evt.data);
      globalListeners.current.forEach((gl) =>
        gl(sockSend, resp.response, resp.data)
      );

      const messageListeners = listeners.current[resp.response];
      if (messageListeners !== undefined)
        messageListeners.forEach((l) => l(sockSend, resp.response, resp.data));
    };

    socket.current = sock;
  }

  const addListener = (m, listener) => {
    const messages = Array.isArray(m) ? m : [m];
    messages.forEach((message) => {
      if (listeners.current[message] === undefined)
        listeners.current[message] = [listener];
      else listeners.current[message].push(listener);
    });
  };

  const addGlobalListener = (listener) => {
    globalListeners.current.push(listener);
  };

  useEffect(() => {
    addListener(
      ["Stdout", "Status", "StatusRet", "RuntimeError", "CompileError"],
      (_send, resp, data) => {
        console.log(`response: ${resp} with data ${JSON.stringify(data)}`);
      }
    );

    sockSend("AddFile", { path: "main.c", data: starter });
  }, []);

  const addFile = (path, contents) => {
    setFiles((f) => {
      const newFiles = {};
      newFiles[path] = contents;
      return { ...f, ...newFiles };
    });
    setCurrentFile(path);
    sockSend("AddFile", { path, data: contents });
  };

  return (
    <FileUploadContext.Provider
      value={{
        files,
        currentFile,
        styles,
        highlight,
        setCurrentFile,
        addFile,
        sockSend,
        addListener,
        addGlobalListener,
      }}
    >
      {children}
    </FileUploadContext.Provider>
  );
};

export const useFileUpload = () => React.useContext(FileUploadContext);
